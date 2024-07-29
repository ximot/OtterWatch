import React, {useEffect, useState} from 'react';
import {Line} from 'react-chartjs-2';
import {Chart, registerables} from 'chart.js';

Chart.register(...registerables);

interface ApiResponse {
    "My CPU Usage": string;
    "Ram Usage": string;
    "Ram Total": string;
    "Ram Available": string;
    "Swap Usage": string;
    "Swap Total": string;
}

interface SystemInfo {
    hostname: string;
    os_name: string;
    kernel_version: string;
    start_time: string;
    cpu_name: string;
    cpu_cores: number;
}

const kilobytesToMegabytes = (kilobytes: number): number => {
    return kilobytes / 1024;
};

const CPUChart: React.FC<{ data: number[], labels: string[] }> = ({data, labels}) => {
    const chartData = {
        labels: labels,
        datasets: [
            {
                label: 'CPU Usage (%)',
                data: data,
                fill: true,
                borderColor: 'rgba(75,192,192,1)',
                backgroundColor: 'rgba(75,192,192,0.2)',
                tension: 0.1,
            },
        ],
    };

    const options = {
        animation: {
            duration: 10,
        },
        easing: 'easeInQuad',
    };

    return (
        <div style={{width: '800px', height: '300px'}}>
            <Line data={chartData} options={options} height={300} width={800}/>
        </div>
    );
};

const RAMChart: React.FC<{
    usageData: number[],
    totalData: number[],
    availableData: number[],
    labels: string[]
}> = ({usageData, totalData, availableData, labels}) => {
    const chartData = {
        labels: labels,
        datasets: [
            {
                label: 'RAM Usage (MB)',
                data: usageData,
                fill: true,
                borderColor: 'rgba(153,102,255,1)',
                backgroundColor: 'rgba(153,102,255,0.2)',
                tension: 0.1,
            },
            {
                label: 'RAM Total (MB)',
                data: totalData,
                fill: false,
                borderColor: 'rgba(255,159,64,1)',
                borderDash: [5, 5], // Dotted line for total
                tension: 0.1,
            },
            {
                label: 'RAM Available (MB)',
                data: availableData,
                fill: false,
                borderColor: 'rgba(75,192,192,1)',
                borderDash: [10, 5], // Dotted line for available RAM
                tension: 0.1,
            },
        ],
    };

    const options = {
        animation: {
            duration: 10,
        },
        easing: 'easeInQuad',
    };

    return (
        <div style={{width: '800px', height: '300px'}}>
            <Line data={chartData} options={options} height={300} width={800}/>
        </div>
    );
};

const SwapChart: React.FC<{ usageData: number[], totalData: number[], labels: string[] }> = ({
                                                                                                 usageData,
                                                                                                 totalData,
                                                                                                 labels
                                                                                             }) => {
    const chartData = {
        labels: labels,
        datasets: [
            {
                label: 'Swap Usage (MB)',
                data: usageData,
                fill: true,
                borderColor: 'rgba(255,99,132,1)',
                backgroundColor: 'rgba(255,99,132,0.2)',
                tension: 0.1,
            },
            {
                label: 'Swap Total (MB)',
                data: totalData,
                fill: false,
                borderColor: 'rgba(54,162,235,1)',
                borderDash: [5, 5], // Dotted line for total
                tension: 0.1,
            },
        ],
    };

    const options = {
        animation: {
            duration: 10,
        },
        easing: 'easeInQuad',
    };

    return (
        <div style={{width: '800px', height: '300px'}}>
            <Line data={chartData} options={options} height={300} width={800}/>
        </div>
    );
};

const App: React.FC = () => {
    const [cpuUsage, setCpuUsage] = useState<number[]>([]);
    const [ramUsage, setRamUsage] = useState<number[]>([]);
    const [ramTotal, setRamTotal] = useState<number[]>([]);
    const [ramAvailable, setRamAvailable] = useState<number[]>([]);
    const [swapUsage, setSwapUsage] = useState<number[]>([]);
    const [swapTotal, setSwapTotal] = useState<number[]>([]);
    const [labels, setLabels] = useState<string[]>([]);
    const [serverName, setServerName] = useState<string>('localhost');
    const [refreshRate, setRefreshRate] = useState<number>(15000); // default 15 seconds
    const [currentServer, setCurrentServer] = useState<string>(`http://${serverName}:8080/system-stats`);
    const [systemInfo, setSystemInfo] = useState<SystemInfo | null>(null);

    useEffect(() => {
        const fetchData = () => {
            fetch(currentServer)
                .then(response => response.json())
                .then((data: ApiResponse) => {
                    const cpu = parseFloat(data["My CPU Usage"]);
                    const ramInKilobytes = parseFloat(data["Ram Usage"]);
                    const ramTotalInKilobytes = parseFloat(data["Ram Total"]);
                    const ramAvailableInKilobytes = parseFloat(data["Ram Available"]);
                    const ramInMB = kilobytesToMegabytes(ramInKilobytes);
                    const ramTotalInMB = kilobytesToMegabytes(ramTotalInKilobytes);
                    const ramAvailableInMB = kilobytesToMegabytes(ramAvailableInKilobytes);
                    const swapInKilobytes = parseFloat(data["Swap Usage"]);
                    const swapTotalInKilobytes = parseFloat(data["Swap Total"]);
                    const swapInMB = kilobytesToMegabytes(swapInKilobytes);
                    const swapTotalInMB = kilobytesToMegabytes(swapTotalInKilobytes);
                    const timestamp = new Date().toLocaleTimeString();

                    setCpuUsage(prevCpu => [...prevCpu, cpu].slice(-240));
                    setRamUsage(prevRam => [...prevRam, ramInMB].slice(-240));
                    setRamTotal(prevRamTotal => [...prevRamTotal, ramTotalInMB].slice(-240));
                    setRamAvailable(prevRamAvailable => [...prevRamAvailable, ramAvailableInMB].slice(-240));
                    setSwapUsage(prevSwap => [...prevSwap, swapInMB].slice(-240));
                    setSwapTotal(prevSwapTotal => [...prevSwapTotal, swapTotalInMB].slice(-240));
                    setLabels(prevLabels => [...prevLabels, timestamp].slice(-240));
                })
                .catch(error => console.error('Error fetching data:', error));
        };

        fetchData();
        const intervalId = setInterval(fetchData, refreshRate);

        return () => clearInterval(intervalId);
    }, [currentServer, refreshRate]);

    const fetchSystemInfo = () => {
        const url = `http://${serverName}:8080/system-info`;
        fetch(url)
            .then(response => response.json())
            .then((data: SystemInfo) => setSystemInfo(data))
            .catch(error => console.error('Error fetching system info:', error));
    };

    const handleServerChange = (event: React.ChangeEvent<HTMLInputElement>) => {
        setServerName(event.target.value);
    };

    const handleServerSubmit = () => {
        setCurrentServer(`http://${serverName}:8080/system-stats`);
        fetchSystemInfo(); // Fetch system info once when the server is set
        // Clear existing data
        setCpuUsage([]);
        setRamUsage([]);
        setRamTotal([]);
        setRamAvailable([]);
        setSwapUsage([]);
        setSwapTotal([]);
        setLabels([]);
    };

    const handleRefreshRateChange = (event: React.ChangeEvent<HTMLSelectElement>) => {
        setRefreshRate(parseInt(event.target.value));
    };

    return (
        <div className="App">
            <h1>System Usage</h1>
            <div>
                <label>
                    Server Name:
                    <input
                        type="text"
                        value={serverName}
                        onChange={handleServerChange}
                        placeholder="Enter server name"
                    />
                </label>
                <button onClick={handleServerSubmit}>Set Server</button>
            </div>
            <div>
                <label>
                    Refresh Rate:
                    <select value={refreshRate} onChange={handleRefreshRateChange}>
                        <option value={1000}>1 second</option>
                        <option value={5000}>5 seconds</option>
                        <option value={15000}>15 seconds</option>
                    </select>
                </label>
            </div>
            <div>
                <h2>System Information</h2>
                {systemInfo && (
                    <div>
                        <p>Hostname: {systemInfo.hostname}</p>
                        <p>OS: {systemInfo.os_name}</p>
                        <p>Kernel Version: {systemInfo.kernel_version}</p>
                        <p>Start Time: {new Date(systemInfo.start_time).toLocaleString()}</p>
                        <p>CPU: {systemInfo.cpu_name}</p>
                        <p>CPU Cores: {systemInfo.cpu_cores}</p>
                    </div>
                )}
            </div>
            <div style={{display: 'flex', justifyContent: 'space-between', margin: '25px'}}>
                <div>
                    <h2>CPU Usage</h2>
                    <CPUChart data={cpuUsage} labels={labels}/>
                </div>
                <div>
                    <h2>RAM Usage</h2>
                    <RAMChart usageData={ramUsage} totalData={ramTotal} availableData={ramAvailable} labels={labels}/>
                </div>
            </div>
            <div style={{display: 'flex', justifyContent: 'space-between', margin: '25px'}}>
                <div>
                    <h2>Swap Usage</h2>
                    <SwapChart usageData={swapUsage} totalData={swapTotal} labels={labels}/>
                </div>
            </div>
        </div>
    );
};

export default App;
