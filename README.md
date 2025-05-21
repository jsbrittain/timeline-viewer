# Timeline Viewer

Timeline viewer for process, thread and GPU (general usage) monitoring. This is designed to collect snapshots over prolonged periods of time (hours of data). Assumes NVIDIA graphics cards.

For GPU profiling I recommend using [NVIDIA Nsight Systems](https://developer.nvidia.com/nsight-systems) or [NVIDIA Nsight Compute](https://developer.nvidia.com/nsight-compute).

## Usage

There is a `monitor` component which probes a process at regular intervals (e.g. 1 sec), and a `timeline_viewer` component which visualizes the data collected by the monitor.

To monitor a process:

```bash
MONITOR_PID=<pid> python3 monitor.py
```

To visualize the data you need to build the `timeline_viewer` component. From the `timeline_viewer` folder, run:

```bash
cargo install trunk
rustup target add wasm32-unknown-unknown
cargo install wasm-bindgen-cli

# Verify install
trunk --version
```

To start the timeline viewer, run:

```bash
trunk serve --open
```

There is a sample file that you can use to test the viewer in `samples` (stored using GitHub LFS).
