![OtterWatch](media/logo.png)
## OtterWatch - Performance monitoring for the Linux operating system

*Application under development*

### Current functionality:

- CPU statistics broken down by I/O wait for CPU
- RAM and SWAP memory usage
- disk operations (read, write, I/O wait time, I/O operation time)
- network statistics (possibility to exclude unwanted network cards, e.g. virtual ones)
- writing of metrics every second to the local database
- access to metrics via web API

### Key assumptions for the project:

- minimal resource consumption
- metrics to locate performance problems (not something that just looks good)
- automatic detection of anomalies in system operation
- and most importantly for me to learn the RUST language :)

(C) 2024 Tomasz Wyderka  