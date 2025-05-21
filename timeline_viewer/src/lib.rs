use gloo_file::callbacks::{read_as_text, FileReader};
use gloo_file::File;
use indexmap::IndexMap;
use js_sys::eval;
use serde::Deserialize;
use std::collections::HashSet;
use std::rc::Rc;
use wasm_bindgen::prelude::wasm_bindgen;
use web_sys::{HtmlElement, HtmlInputElement};
use yew::prelude::*;

#[allow(non_snake_case)]
#[derive(Debug, Clone, PartialEq, Deserialize)]
struct Snapshot {
    Timestamp: String,
    ProcessTree: Process,
    #[serde(default)]
    GPUStatus: Vec<GPUStatus>,
    #[serde(default)]
    CPU_Cores_Total: u32,
}

#[allow(non_snake_case)]
#[derive(Debug, Clone, PartialEq, Deserialize)]
struct Process {
    PID: u32,
    Name: String,
    CMD: Option<String>,
    Threads: Option<Vec<Thread>>,
    Children: Option<Vec<Process>>,
}

#[allow(non_snake_case)]
#[derive(Debug, Clone, PartialEq, Deserialize)]
struct Thread {
    TID: u32,
    Name: Option<String>,
    State: Option<String>,
}

#[allow(non_snake_case)]
#[derive(Debug, Clone, PartialEq, Deserialize)]
struct GPUStatus {
    GPU_ID: u32,
    Name: String,
    Load_Percent: f64,
    Memory_Used_MB: f64,
    Memory_Total_MB: f64,
    Temperature_C: f64,
    Driver: String,
}

fn count_running_threads(proc: &Process) -> usize {
    let mut count = 0;

    if let Some(threads) = &proc.Threads {
        for t in threads {
            if let Some(state) = &t.State {
                if state.starts_with('R') {
                    count += 1;
                }
            }
        }
    }

    if let Some(children) = &proc.Children {
        for child in children {
            count += count_running_threads(child);
        }
    }

    count
}

fn walk(
    proc: &Process,
    timestamp: usize,
    label_map: &IndexMap<String, usize>,
    matrix: &mut Vec<(usize, usize, u8)>,
    depth: usize,
) {
    let indent = "    ".repeat(depth);
    let proc_label = if depth == 0 {
        format!("{indent}{} (PID {})", proc.Name, proc.PID)
    } else {
        format!("{indent}└─ {} (PID {})", proc.Name, proc.PID)
    };
    if let Some(&row) = label_map.get(&proc_label) {
        matrix.push((timestamp, row, 1));
    }

    if let Some(threads) = &proc.Threads {
        for t in threads {
            let indent = "    ".repeat(depth + 1);
            let tid_label = format!(
                "{indent}└─ {} (TID {})",
                t.Name.clone().unwrap_or_default(),
                t.TID
            );
            if let Some(&row) = label_map.get(&tid_label) {
                let val = match t
                    .State
                    .clone()
                    .unwrap_or_default()
                    .chars()
                    .next()
                    .unwrap_or('-')
                {
                    'R' => 1,
                    'S' => 2,
                    'Z' => 3,
                    'T' => 4,
                    _ => 0,
                };
                matrix.push((timestamp, row, val));
            }
        }
    }

    if let Some(children) = &proc.Children {
        for child in children {
            walk(child, timestamp, label_map, matrix, depth + 1);
        }
    }
}

#[function_component(App)]
fn app() -> Html {
    let chart_ref = use_node_ref();
    let reader_handle = use_state(|| None::<FileReader>);
    let snapshots = use_state(|| Rc::new(Vec::<Snapshot>::new()));
    let file_input_ref = use_node_ref();
    let min_time = use_state(|| 0);
    let max_time = use_state(|| 0);

    let on_file_change = {
        let snapshots = snapshots.clone();
        let reader_handle = reader_handle.clone();
        let min_time = min_time.clone();
        let max_time = max_time.clone();
        Callback::from(move |event: Event| {
            let input: HtmlInputElement = event.target_unchecked_into();
            if let Some(files) = input.files() {
                if let Some(file) = files.get(0) {
                    let file = File::from(file);
                    let snapshots = snapshots.clone();
                    let reader_handle = reader_handle.clone();
                    let min_time = min_time.clone();
                    let max_time = max_time.clone();

                    let reader = read_as_text(&file, move |res: Result<String, _>| {
                        if let Ok(content) = res {
                            let mut parsed = Vec::new();
                            for line in content.lines() {
                                match serde_json::from_str::<Snapshot>(line) {
                                    Ok(snapshot) => parsed.push(snapshot),
                                    Err(e) => {
                                        gloo::console::log!(format!("Failed to parse line: {}", e))
                                    }
                                }
                            }
                            let len = parsed.len();
                            min_time.set(0);
                            max_time.set(len.saturating_sub(1));
                            snapshots.set(Rc::new(parsed));
                            gloo::console::log!("Snapshots loaded");
                        }
                    });
                    reader_handle.set(Some(reader));
                }
            }
        })
    };

    use_effect_with(
        (
            snapshots.clone(),
            chart_ref.clone(),
            min_time.clone(),
            max_time.clone(),
        ),
        move |(snapshots, chart_ref, min_time, max_time)| {
            if snapshots.is_empty() || chart_ref.get().is_none() {
                return;
            }

            #[derive(Debug)]
            struct LabelNode {
                label: String,
                children: IndexMap<String, LabelNode>,
            }

            fn insert_process(node: &mut LabelNode, proc: &Process, depth: usize) {
                let indent = "    ".repeat(depth);
                let proc_label = if depth == 0 {
                    format!("{indent}{} (PID {})", proc.Name, proc.PID)
                } else {
                    format!("{indent}└─ {} (PID {})", proc.Name, proc.PID)
                };

                let child_node = node
                    .children
                    .entry(proc_label.clone())
                    .or_insert(LabelNode {
                        label: proc_label.clone(),
                        children: IndexMap::new(),
                    });

                if let Some(threads) = &proc.Threads {
                    for t in threads {
                        let indent = "    ".repeat(depth + 1);
                        let tid_label = format!(
                            "{indent}└─ {} (TID {})",
                            t.Name.clone().unwrap_or_default(),
                            t.TID
                        );
                        child_node
                            .children
                            .entry(tid_label.clone())
                            .or_insert(LabelNode {
                                label: tid_label,
                                children: IndexMap::new(),
                            });
                    }
                }

                if let Some(children) = &proc.Children {
                    for child in children {
                        insert_process(child_node, child, depth + 1);
                    }
                }
            }

            fn flatten_tree(node: &LabelNode, label_order: &mut Vec<String>) {
                if !node.label.is_empty() {
                    label_order.push(node.label.clone());
                }
                for child in node.children.values() {
                    flatten_tree(child, label_order);
                }
            }

            let min = **min_time;
            let max = **max_time;

            // Build process/thread hierarchy tree
            let mut root = LabelNode {
                label: String::new(),
                children: IndexMap::new(),
            };

            // Collect GPU labels before flattening
            let mut gpu_labels = HashSet::new();
            for snap in snapshots.iter() {
                for gpu in &snap.GPUStatus {
                    let label = format!("GPU #{}", gpu.GPU_ID);
                    gpu_labels.insert(label);
                }
            }
            let mut gpu_labels: Vec<String> = gpu_labels.into_iter().collect();
            gpu_labels.sort();

            for snap in snapshots.iter() {
                insert_process(&mut root, &snap.ProcessTree, 0);
            }

            // Build label order: GPU labels first, then hierarchical processes
            let mut label_order = gpu_labels;
            flatten_tree(&root, &mut label_order);
            let label_map: IndexMap<String, usize> = label_order
                .iter()
                .cloned()
                .enumerate()
                .map(|(i, s)| (s, i))
                .collect();

            // Step 4: Build matrix
            let mut matrix = Vec::new();

            for (timestamp_index, snap) in
                snapshots.iter().enumerate().skip(min).take(max - min + 1)
            {
                walk(
                    &snap.ProcessTree,
                    timestamp_index,
                    &label_map,
                    &mut matrix,
                    0,
                );

                for gpu in snap.GPUStatus.iter() {
                    let label = format!("GPU #{}", gpu.GPU_ID);
                    if let Some(&row) = label_map.get(&label) {
                        // Use colormap indices 5–105 for GPU load gradient
                        let value = gpu.Load_Percent.clamp(0.0, 100.0) as u8 + 5;
                        matrix.push((timestamp_index, row, value));
                    }
                }
            }

            // GPU Trace
            let mut gpu_series_data: IndexMap<u32, Vec<(usize, f64)>> = IndexMap::new();
            for (timestamp_index, snap) in
                snapshots.iter().enumerate().skip(min).take(max - min + 1)
            {
                for gpu in &snap.GPUStatus {
                    gpu_series_data
                        .entry(gpu.GPU_ID)
                        .or_default()
                        .push((timestamp_index, gpu.Load_Percent));
                }
            }
            let gpu_line_series: Vec<_> = gpu_series_data
                .into_iter()
                .map(|(gpu_id, data)| {
                    let points: Vec<(usize, f64)> = data;
                    format!(
                        r#"{{
                            name: "GPU #{gpu_id}",
                            type: "line",
                            data: {},
                            showSymbol: false
                        }}"#,
                        serde_json::to_string(&points).unwrap()
                    )
                })
                .collect();

            let gpu_line_series_str = format!("[{}]", gpu_line_series.join(","));

            // CPU Trace
            let mut cpu_trace: Vec<(usize, f64)> = Vec::new();
            for (timestamp_index, snap) in
                snapshots.iter().enumerate().skip(min).take(max - min + 1)
            {
                let running_threads = count_running_threads(&snap.ProcessTree);
                let total_cores = snap.CPU_Cores_Total.max(1); // prevent division by 0
                let cpu_percent = (running_threads as f64 / total_cores as f64) * 100.0;
                cpu_trace.push((timestamp_index, cpu_percent));
            }

            // GPU memory percentage
            let mut gpu_mem_series_data: IndexMap<u32, Vec<(usize, f64)>> = IndexMap::new();

            for (timestamp_index, snap) in
                snapshots.iter().enumerate().skip(min).take(max - min + 1)
            {
                for gpu in &snap.GPUStatus {
                    let percent_used = if gpu.Memory_Total_MB > 0.0 {
                        (gpu.Memory_Used_MB / gpu.Memory_Total_MB) * 100.0
                    } else {
                        0.0
                    };
                    gpu_mem_series_data
                        .entry(gpu.GPU_ID)
                        .or_default()
                        .push((timestamp_index, percent_used));
                }
            }
            let gpu_mem_line_series: Vec<_> = gpu_mem_series_data
                .into_iter()
                .map(|(gpu_id, data)| {
                    let points: Vec<(usize, f64)> = data;
                    format!(
                        r#"{{
                            name: "GPU #{gpu_id} Mem %",
                            type: "line",
                            data: {},
                            showSymbol: false,
                        }}"#,
                        serde_json::to_string(&points).unwrap()
                    )
                })
                .collect();

            let gpu_mem_line_series_str = format!("[{}]", gpu_mem_line_series.join(","));

            // Render chart
            let height = label_map.len() * 14;
            let x_labels: Vec<String> = (min..=max).map(|i| format!("T{i}")).collect();
            let y_labels: Vec<String> = label_order;

            if let Some(div) = chart_ref.cast::<HtmlElement>() {
                div.style()
                    .set_property("height", &format!("{}px", height))
                    .unwrap();

                let js_code = format!(
                    r#"
                        setTimeout(() => {{
                            const dom = document.getElementById('heatmap');
                            if (!dom) return;
                            if (echarts.getInstanceByDom(dom)) {{
                                echarts.dispose(dom);
                            }}
                            const chart = echarts.init(dom);
                            const option = {{
                                tooltip: {{
                                    formatter: function (p) {{
                                        const val = p.data[2];
                                        if (val > 5) {{
                                            return `Time: ${{p.data[0]}}<br/>GPU Load: ${{Math.round(val - 5)}}%`;
                                        }} else {{
                                            const state = ['-', 'R', 'S', 'Z', 'T'][val] || '?';
                                            return `Time: ${{p.data[0]}}<br/>Thread State: ${{state}}`;
                                        }}
                                    }}
                                }},
                                grid: {{ height: '80%', top: '10%', left: 300 }},
                                xAxis: {{ type: 'category', data: {xdata}, splitArea: {{ show: true }} }},
                                yAxis: {{
                                    type: 'category',
                                    data: {ydata},
                                    splitArea: {{ show: true }},
                                    axisLabel: {{ interval: 0, align: 'left', margin: 300 }},
                                    inverse: true
                                }},
                                visualMap: {{
                                    type: 'piecewise',
                                    dimension: 2,
                                    show: true,
                                    calculable: true,
                                    top: 'center',
                                    left: 'right',
                                    pieces: [
                                        {{ min: 0, max: 0, label: 'Unknown', color: 'white' }},
                                        {{ min: 1, max: 1, label: 'Running (R)', color: 'green' }},
                                        {{ min: 2, max: 2, label: 'Sleeping (S)', color: 'orange' }},
                                        {{ min: 3, max: 3, label: 'Zombie (Z)', color: 'red' }},
                                        {{ min: 4, max: 4, label: 'Stopped (T)', color: 'gray' }},

                                        // GPU values bucketed manually
                                        {{ min: 5, max: 20, label: 'GPU 0–15%', color: '#e0f3f8' }},
                                        {{ min: 21, max: 40, label: 'GPU 16–35%', color: '#abd9e9' }},
                                        {{ min: 41, max: 60, label: 'GPU 36–55%', color: '#74add1' }},
                                        {{ min: 61, max: 80, label: 'GPU 56–75%', color: '#4575b4' }},
                                        {{ min: 81, max: 105, label: 'GPU 76–100%', color: '#313695' }}
                                    ]
                                }},
                                series: [{{
                                    name: 'State',
                                    type: 'heatmap',
                                    data: {matrix},
                                    label: {{ show: false }},
                                    emphasis: {{
                                        itemStyle: {{
                                            shadowBlur: 10,
                                            shadowColor: 'rgba(0, 0, 0, 0.5)'
                                        }}
                                    }}
                                }}]
                            }};
                            chart.setOption(option);

                            // === GPU Line Chart ===
                            const dom2 = document.getElementById('gpu-load-line');
                            if (!dom2) return;
                            if (echarts.getInstanceByDom(dom2)) {{
                                echarts.dispose(dom2);
                            }}
                            const chart2 = echarts.init(dom2);
                            const option2 = {{
                                title: {{ text: 'GPU Load Over Time (%)' }},
                                tooltip: {{ trigger: 'axis' }},
                                legend: {{ top: 20 }},
                                xAxis: {{
                                    type: 'category',
                                    data: {xdata}
                                }},
                                yAxis: {{
                                    type: 'value',
                                    min: 0,
                                    max: 100,
                                    axisLabel: {{ formatter: '{{value}}%' }}
                                }},
                                series: {gpu_line_series}
                            }};
                            chart2.setOption(option2);

                            // === CPU Line Chart ===
                            const dom3 = document.getElementById('cpu-load-line');
                            if (dom3) {{
                                if (echarts.getInstanceByDom(dom3)) {{
                                    echarts.dispose(dom3);
                                }}
                                const chart3 = echarts.init(dom3);
                                const option3 = {{
                                    title: {{ text: 'CPU Utilization Over Time (%)' }},
                                    tooltip: {{ trigger: 'axis' }},
                                    xAxis: {{
                                        type: 'category',
                                        data: {xdata}
                                    }},
                                    yAxis: {{
                                        type: 'value',
                                        min: 0,
                                        max: 100,
                                        axisLabel: {{ formatter: '{{value}}%' }}
                                    }},
                                series: [{{
                                        name: 'CPU Utilization',
                                        type: 'line',
                                        data: {cpu_data},
                                        showSymbol: false,
                                    }}]
                                }};
                                chart3.setOption(option3);
                            }}


                            // === GPU Memory Line Chart ===
                            const dom4 = document.getElementById('gpu-mem-line');
                            if (dom4) {{
                                if (echarts.getInstanceByDom(dom4)) {{
                                    echarts.dispose(dom4);
                                }}
                                const chart4 = echarts.init(dom4);
                                const option4 = {{
                                    title: {{ text: 'GPU Memory Usage Over Time (%)' }},
                                    tooltip: {{ trigger: 'axis' }},
                                    legend: {{ top: 20 }},
                                    xAxis: {{
                                        type: 'category',
                                        data: {xdata}
                                    }},
                                    yAxis: {{
                                        type: 'value',
                                        min: 0,
                                        max: 100,
                                        axisLabel: {{ formatter: '{{value}}%' }}
                                    }},
                                    series: {gpu_mem_series}
                                }};
                                chart4.setOption(option4);
                            }}
                        }}, 0);
                    "#,
                    xdata = serde_json::to_string(&x_labels).unwrap(),
                    ydata = serde_json::to_string(&y_labels).unwrap(),
                    matrix = serde_json::to_string(&matrix).unwrap(),
                    gpu_line_series = gpu_line_series_str,
                    cpu_data = serde_json::to_string(&cpu_trace).unwrap(),
                    gpu_mem_series = gpu_mem_line_series_str,
                );

                let _ = eval(&js_code);
            }
        },
    );
    html! {
        <div style="padding: 2em;">
            <input type="file" accept=".jsonl" ref={file_input_ref} onchange={on_file_change} />
            <p>{ format!("Time range: {} - {}", *min_time, *max_time) }</p>
            <input type="range" min="0" max={(*max_time).to_string()} value={(*min_time).to_string()} oninput={{
                let min_time = min_time.clone();
                Callback::from(move |e: InputEvent| {
                    let input: HtmlInputElement = e.target_unchecked_into();
                    if let Ok(value) = input.value().parse::<usize>() {
                        min_time.set(value);
                    }
                })
            }} />
            <input type="range" min="0" max={(*max_time).to_string()} value={(*max_time).to_string()} oninput={{{
                let max_time = max_time.clone();
                Callback::from(move |e: InputEvent| {
                    let input: HtmlInputElement = e.target_unchecked_into();
                    if let Ok(value) = input.value().parse::<usize>() {
                        max_time.set(value);
                    }
                })
            }}} />
            <div id="heatmap" ref={chart_ref} style="width:100%;" />
            <div id="gpu-load-line" style="width:100%; height:300px; margin-top:2em;" />
            <div id="gpu-mem-line" style="width:100%; height:300px; margin-top:2em;" />
            <div id="cpu-load-line" style="width:100%; height:300px; margin-top:2em;" />
        </div>
    }
}

#[wasm_bindgen(start)]
pub fn start() {
    gloo::console::log!("ECharts Heatmap Viewer booting...");
    yew::Renderer::<App>::new().render();
}
