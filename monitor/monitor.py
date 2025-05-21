import os
import psutil
import json
import time
from datetime import datetime
import subprocess
import GPUtil

# Set the root PID to monitor via environment variable
ROOT_PID = int(os.getenv("MONITOR_PID", "1"))

LOG_FILE = f"monitor_logs/{ROOT_PID}.jsonl"
INTERVAL = 1  # seconds
os.makedirs(os.path.dirname(LOG_FILE), exist_ok=True)


def get_thread_info(pid):
    threads = []
    task_dir = f"/proc/{pid}/task"
    if not os.path.exists(task_dir):
        return threads
    for tid in os.listdir(task_dir):
        status_path = os.path.join(task_dir, tid, "status")
        try:
            with open(status_path) as f:
                thread_data = {
                    "TID": int(tid),
                    "Name": None,
                    "State": None,
                    "CPU_Affinity": None,
                    "Core": None,
                }
                for line in f:
                    if line.startswith("Name:"):
                        thread_data["Name"] = line.split(":")[1].strip()
                    elif line.startswith("State:"):
                        thread_data["State"] = line.split(":")[1].strip()
                    elif line.startswith("Cpus_allowed_list:"):
                        thread_data["CPU_Affinity"] = line.split(":")[1].strip()

                stat_path = os.path.join(task_dir, tid, "stat")
                try:
                    with open(stat_path) as f_stat:
                        fields = f_stat.read().split()
                        if len(fields) >= 39:
                            thread_data["Core"] = int(fields[38])
                except FileNotFoundError:
                    thread_data["Core"] = None

                threads.append(thread_data)
        except FileNotFoundError:
            continue
    return threads


def get_gpu_info():
    gpus = GPUtil.getGPUs()
    gpu_data = []
    for gpu in gpus:
        gpu_data.append({
            "GPU_ID": gpu.id,
            "Name": gpu.name,
            "Load_Percent": round(gpu.load * 100, 1),
            "Memory_Used_MB": gpu.memoryUsed,
            "Memory_Total_MB": gpu.memoryTotal,
            "Temperature_C": gpu.temperature,
            "Driver": gpu.driver
        })
    return gpu_data


def get_gpu_process_details():
    try:
        output = subprocess.check_output([
            "nvidia-smi",
            "--query-compute-apps=pid,process_name,gpu_uuid,used_memory",
            "--format=csv,noheader,nounits"
        ], encoding="utf-8")
        result = []
        for line in output.strip().split("\n"):
            if not line:
                continue
            pid, name, uuid, mem = [x.strip() for x in line.split(",")]
            result.append({
                "PID": int(pid),
                "Process_Name": name,
                "GPU_UUID": uuid,
                "GPU_Memory_MB": int(mem)
            })
        return result
    except subprocess.CalledProcessError:
        return []


def snapshot_system_state(root_pid):
    try:
        root_proc = psutil.Process(root_pid)
        proc_info = {
            "PID": root_proc.pid,
            "Name": root_proc.name(),
            "CMD": ' '.join(root_proc.cmdline()) if root_proc.cmdline() else root_proc.name(),
            "Threads": get_thread_info(root_proc.pid),
            "Children": []
        }
        for child in root_proc.children(recursive=True):
            child_data = {
                "PID": child.pid,
                "Name": child.name(),
                "CMD": ' '.join(child.cmdline()) if child.cmdline() else child.name(),
                "Threads": get_thread_info(child.pid)
            }
            proc_info["Children"].append(child_data)
        return {
            "Timestamp": datetime.now().isoformat(),
            "CPU_Cores_Total": os.cpu_count(),
            "ProcessTree": proc_info,
            "GPUStatus": get_gpu_info(),
            "GPUProcesses": get_gpu_process_details()
        }
    except psutil.NoSuchProcess:
        return None


def capture_loop():
    print(f"[INFO] Logging to {LOG_FILE} every {INTERVAL}s... Press Ctrl+C to stop.")
    try:
        with open(LOG_FILE, "a") as log_file:
            while True:
                snapshot = snapshot_system_state(ROOT_PID)
                if not snapshot:
                    print("Process no longer running. Exiting.")
                    break
                log_file.write(json.dumps(snapshot) + "\n")
                log_file.flush()
                time.sleep(INTERVAL)
    except KeyboardInterrupt:
        print("[INFO] Logging stopped by user.")


if __name__ == "__main__":
    capture_loop()
