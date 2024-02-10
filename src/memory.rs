use std::fs;

pub fn read_memory_info() -> (u64, u64, u64, u64, u64) {
    let content = fs::read_to_string("/proc/meminfo").unwrap();
    let mut mem_total = 0;
    let mut mem_free = 0;
    let mut mem_aval = 0;
    let mut swap_total = 0;
    let mut swap_free = 0;

    for line in content.lines() {
        match line.split_whitespace().collect::<Vec<&str>>().as_slice() {
            ["MemTotal:", total, ..] => mem_total = total.parse().unwrap(),
            ["MemFree:", free, ..] => mem_free = free.parse().unwrap(),
            ["MemAvailable:", aval, ..] => mem_aval = aval.parse().unwrap(),
            ["SwapTotal:", total, ..] => swap_total = total.parse().unwrap(),
            ["SwapFree:", free, ..] => swap_free = free.parse().unwrap(),
            _ => {}
        }
    }

    (mem_total, mem_free, mem_aval, swap_total, swap_free)
}
