# OtterWatch Plugins - Dokumentacja

## Spis treści

1. [Wprowadzenie](#wprowadzenie)
2. [Architektura systemu pluginów](#architektura-systemu-pluginów)
3. [Dostępne pluginy](#dostępne-pluginy)
4. [Konfiguracja](#konfiguracja)
5. [Zbierane metryki](#zbierane-metryki)
6. [Źródła danych](#źródła-danych)
7. [Tworzenie nowego pluginu](#tworzenie-nowego-pluginu)
8. [API i struktury danych](#api-i-struktury-danych)

---

## Wprowadzenie

System pluginów w OtterWatch umożliwia monitorowanie konkretnych usług systemowych (np. nginx, Tomcat) niezależnie od ogólnych metryk systemowych. Pluginy dostarczają szczegółowe informacje o:

- Zużyciu CPU przez usługę
- Wykorzystaniu pamięci (RAM, swap)
- Operacjach I/O dyskowych
- Liczbie procesów i wątków
- Szczegółach poszczególnych procesów usługi

### Kiedy używać pluginów?

- Gdy chcesz monitorować konkretną usługę osobno od całego systemu
- Gdy potrzebujesz szczegółowych metryk dla aplikacji (np. heap memory Java)
- Gdy chcesz śledzić wydajność wielu instancji tej samej usługi
- Gdy potrzebujesz izolowanych metryk dla serwisów działających w systemd

---

## Architektura systemu pluginów

```
┌─────────────────────────────────────────────────────────────┐
│                      PluginRegistry                          │
│  ┌──────────────┐ ┌──────────────┐ ┌──────────────────────┐ │
│  │ NginxPlugin  │ │ TomcatPlugin │ │ SelfMonitorPlugin    │ │
│  └──────┬───────┘ └──────┬───────┘ └──────────┬───────────┘ │
│         │                │                     │             │
│         └────────────────┼─────────────────────┘             │
│                          │                                   │
│                          ▼                                   │
│              ┌───────────────────────┐                      │
│              │   ServicePlugin trait │                      │
│              │   • name()            │                      │
│              │   • is_available()    │                      │
│              │   • collect()         │                      │
│              └───────────┬───────────┘                      │
└──────────────────────────┼──────────────────────────────────┘
                           │
           ┌───────────────┼───────────────┐
           ▼               ▼               ▼
    ┌────────────┐  ┌────────────┐  ┌────────────┐
    │  cgroup    │  │   /proc    │  │ /proc/self │
    │  v2 / v1   │  │  scanning  │  │   (self)   │
    └────────────┘  └────────────┘  └────────────┘
```

### Główne komponenty

| Komponent | Plik | Opis |
|-----------|------|------|
| `ServicePlugin` | `mod.rs` | Trait definiujący interfejs pluginu |
| `PluginRegistry` | `mod.rs` | Rejestr wszystkich dostępnych pluginów |
| `PluginConfig` | `mod.rs` | Konfiguracja pojedynczego pluginu |
| `PluginSettings` | `mod.rs` | Ustawienia wszystkich pluginów z settings.toml |
| `PluginState` | `mod.rs` | Stan między cyklami zbierania (dla obliczeń delta) |
| `ServiceMetrics` | `mod.rs` | Struktura wyjściowa z metrykami usługi |
| `cgroup` | `cgroup.rs` | Moduł odczytu metryk z cgroups |

---

## Dostępne pluginy

### 1. nginx

**Plik:** `src/plugins/nginx.rs`

Monitoruje serwer WWW nginx.

**Wzorce wykrywania procesów:**
```regex
^nginx:     # Procesy nginx (master/worker)
^nginx$     # Proces o nazwie dokładnie "nginx"
```

**Źródło danych:**
1. Cgroup (preferowane) - `/sys/fs/cgroup/system.slice/nginx.service/`
2. Procfs (fallback) - skanowanie `/proc/[pid]/` dla pasujących procesów

### 2. tomcat

**Plik:** `src/plugins/tomcat.rs`

Monitoruje serwer aplikacji Apache Tomcat (Java).

**Wzorce wykrywania procesów:**
```regex
java.*tomcat                           # Java z "tomcat" w argumentach
java.*catalina                         # Java z "catalina" w argumentach
org\.apache\.catalina\.startup\.Bootstrap  # Główna klasa bootstrapera
```

**Źródło danych:**
1. Cgroup (preferowane) - `/sys/fs/cgroup/system.slice/tomcat.service/`
2. Procfs (fallback) - skanowanie procesów Java z Tomcat w cmdline

### 3. self_monitor

**Plik:** `src/plugins/self_monitor.rs`

Monitoruje samego agenta OtterWatch.

**Unikalne funkcje:**
- Zawsze dostępny (czyta `/proc/self`)
- Opcjonalne zbieranie liczby otwartych deskryptorów plików
- Opcjonalne zbieranie statystyk I/O

**Źródło danych:** Wyłącznie `/proc/self/`

---

## Konfiguracja

### settings.toml

```toml
[plugins]
# Interwał zbierania metryk pluginów (sekundy)
plugin_interval_secs = 10

# Zbieraj szczegóły poszczególnych procesów
collect_process_details = false

# Plugin nginx
[plugins.nginx]
enabled = true
service_name = "nginx"           # Nazwa serwisu systemd
process_patterns = ["^nginx:"]   # Opcjonalne niestandardowe wzorce regex

# Plugin tomcat
[plugins.tomcat]
enabled = true
service_name = "tomcat"
process_patterns = []            # Puste = użyj domyślnych

# Plugin self-monitor
[plugins.self_monitor]
enabled = true
service_name = "otterwatch"
collect_open_fds = true          # Zbieraj liczbę otwartych FD
collect_io_stats = true          # Zbieraj statystyki I/O
```

### Zdalne włączanie pluginów (Dashboard)

Od wersji 0.2.3 pluginy można włączać/wyłączać zdalnie z poziomu dashboardu:

1. Otwórz **Admin Panel**
2. Kliknij przycisk **CONFIG** przy agencie
3. Przewiń do sekcji **Plugins**
4. Kliknij na wartość `enabled` (true/false) aby przełączyć

### Parametry konfiguracyjne

| Parametr | Typ | Zakres | Opis |
|----------|-----|--------|------|
| `plugins.plugin_interval_secs` | int | 5-3600 | Interwał zbierania w sekundach |
| `plugins.collect_process_details` | bool | - | Zbieraj szczegóły procesów |
| `plugins.nginx.enabled` | bool | - | Włącz plugin nginx |
| `plugins.nginx.service_name` | string | - | Nazwa serwisu systemd |
| `plugins.tomcat.enabled` | bool | - | Włącz plugin tomcat |
| `plugins.tomcat.service_name` | string | - | Nazwa serwisu systemd |
| `plugins.self_monitor.enabled` | bool | - | Włącz self-monitoring |
| `plugins.self_monitor.collect_open_fds` | bool | - | Zbieraj liczbę FD |
| `plugins.self_monitor.collect_io_stats` | bool | - | Zbieraj statystyki I/O |

---

## Zbierane metryki

### ServiceMetrics

| Metryka | Typ | Opis |
|---------|-----|------|
| `service_name` | string | Nazwa usługi |
| `plugin_type` | string | Typ pluginu (nginx, tomcat, self) |
| `is_running` | bool | Czy usługa działa |
| **CPU** | | |
| `cpu_usage_usec` | u64 | Całkowity czas CPU (mikrosekundy) |
| `cpu_percent` | f64 | Procent użycia CPU (delta) |
| `cpu_user_usec` | u64 | Czas CPU w trybie użytkownika |
| `cpu_system_usec` | u64 | Czas CPU w trybie jądra |
| **Pamięć** | | |
| `memory_current_bytes` | u64 | Aktualne użycie RAM |
| `memory_swap_bytes` | u64 | Użycie swap |
| `memory_anon_bytes` | u64 | Pamięć anonimowa (heap, stack) |
| `memory_file_bytes` | u64 | Pamięć cache plików |
| **Disk I/O** | | |
| `disk_read_bytes` | u64 | Bajty odczytane z dysku |
| `disk_write_bytes` | u64 | Bajty zapisane na dysk |
| `disk_read_ops` | u64 | Liczba operacji odczytu |
| `disk_write_ops` | u64 | Liczba operacji zapisu |
| **Network I/O** | | |
| `net_rx_bytes` | u64 | Bajty odebrane (tylko cgroup) |
| `net_tx_bytes` | u64 | Bajty wysłane (tylko cgroup) |
| **Procesy** | | |
| `process_count` | u32 | Liczba procesów usługi |
| `thread_count` | u64 | Łączna liczba wątków |
| `processes` | Vec | Szczegóły procesów (opcjonalne) |
| **Metadata** | | |
| `cgroup_version` | Option<u8> | Wersja cgroup (1, 2, None) |
| `data_source` | enum | Źródło danych |

### ProcessMetrics (szczegóły procesu)

| Pole | Typ | Opis |
|------|-----|------|
| `pid` | u32 | ID procesu |
| `name` | string | Nazwa procesu |
| `cpu_percent` | f64 | Procent CPU tego procesu |
| `memory_bytes` | u64 | RSS w bajtach |
| `threads` | u64 | Liczba wątków |

---

## Źródła danych

### Priorytet źródeł

1. **Cgroup v2** (preferowane) - najdokładniejsze, agregowane przez kernel
2. **Cgroup v1** (legacy) - obsługa starszych systemów
3. **Procfs** (fallback) - skanowanie `/proc/[pid]/`

### Cgroup v2

Ścieżka: `/sys/fs/cgroup/system.slice/{service_name}.service/`

| Plik | Metryki |
|------|---------|
| `cpu.stat` | usage_usec, user_usec, system_usec |
| `memory.current` | current_bytes |
| `memory.swap.current` | swap_bytes |
| `memory.stat` | anon, file, kernel, shmem |
| `io.stat` | rbytes, wbytes, rios, wios |
| `pids.current` | process_count |
| `cgroup.threads` | thread_count |

### Cgroup v1

Ścieżki rozdzielone po kontrolerach:
- `/sys/fs/cgroup/cpu/system.slice/{service}.service/`
- `/sys/fs/cgroup/memory/system.slice/{service}.service/`
- `/sys/fs/cgroup/blkio/system.slice/{service}.service/`

### Procfs (fallback)

Skanowanie `/proc/[pid]/` dla procesów pasujących do wzorców:

| Plik | Dane |
|------|------|
| `comm` | Nazwa procesu |
| `cmdline` | Pełna linia poleceń |
| `stat` | CPU ticks, liczba wątków |
| `statm` | RSS w stronach pamięci |
| `io` | read_bytes, write_bytes |

---

## Tworzenie nowego pluginu

### Krok 1: Struktura pluginu

Utwórz plik `src/plugins/myservice.rs`:

```rust
//! My Service monitoring plugin.

use super::cgroup;
use super::CgroupVersion;
use super::{DataSource, PluginConfig, PluginState, ProcessMetrics, ServiceMetrics, ServicePlugin};
use regex::Regex;
use std::fs;
use std::io;
use std::path::Path;

pub struct MyServicePlugin {
    patterns: Vec<Regex>,
}

impl MyServicePlugin {
    pub fn new() -> Self {
        Self {
            patterns: vec![
                Regex::new(r"myservice").unwrap(),
            ],
        }
    }
}

impl Default for MyServicePlugin {
    fn default() -> Self {
        Self::new()
    }
}
```

### Krok 2: Implementacja traitu ServicePlugin

```rust
impl ServicePlugin for MyServicePlugin {
    fn name(&self) -> &'static str {
        "myservice"
    }

    fn is_available(&self, config: &PluginConfig) -> bool {
        // Sprawdź czy usługa jest dostępna
        if cgroup::cgroup_path_exists(&config.service_name).is_some() {
            return true;
        }
        // Sprawdź procesy jako fallback
        false
    }

    fn collect(
        &self,
        config: &PluginConfig,
        state: &mut PluginState,
        collect_processes: bool,
    ) -> io::Result<ServiceMetrics> {
        // 1. Spróbuj zebrać z cgroup
        if let Some((path, version)) = cgroup::cgroup_path_exists(&config.service_name) {
            if let Ok(metrics) = self.collect_from_cgroup(config, state, &path, version) {
                return Ok(metrics);
            }
        }

        // 2. Fallback do skanowania /proc
        self.collect_from_proc(config, state, collect_processes)
    }
}
```

### Krok 3: Metoda zbierania z cgroup

```rust
impl MyServicePlugin {
    fn collect_from_cgroup(
        &self,
        config: &PluginConfig,
        state: &mut PluginState,
        cgroup_path: &Path,
        version: CgroupVersion,
    ) -> io::Result<ServiceMetrics> {
        let stats = cgroup::read_cgroup_stats(cgroup_path, version)?;

        let now = std::time::Instant::now();
        let elapsed_secs = state
            .last_collection_time
            .map(|t| now.duration_since(t).as_secs_f64())
            .unwrap_or(1.0);
        state.last_collection_time = Some(now);

        let cpu_percent = state.calculate_cpu_percent(stats.cpu.usage_usec, elapsed_secs);

        Ok(ServiceMetrics {
            service_name: config.service_name.clone(),
            plugin_type: "myservice".to_string(),
            is_running: true,
            cpu_usage_usec: stats.cpu.usage_usec,
            cpu_percent,
            cpu_user_usec: stats.cpu.user_usec,
            cpu_system_usec: stats.cpu.system_usec,
            memory_current_bytes: stats.memory.current_bytes,
            // ... pozostałe pola
            ..Default::default()
        })
    }
}
```

### Krok 4: Rejestracja w PluginRegistry

W `src/plugins/mod.rs`:

```rust
pub mod myservice;  // Dodaj eksport modułu

impl PluginRegistry {
    pub fn new() -> Self {
        let mut plugins: HashMap<String, Box<dyn ServicePlugin>> = HashMap::new();

        plugins.insert("nginx".to_string(), Box::new(nginx::NginxPlugin::new()));
        plugins.insert("tomcat".to_string(), Box::new(tomcat::TomcatPlugin::new()));
        plugins.insert("self".to_string(), Box::new(self_monitor::SelfMonitorPlugin::new()));

        // Dodaj nowy plugin
        plugins.insert("myservice".to_string(), Box::new(myservice::MyServicePlugin::new()));

        Self { plugins }
    }
}
```

### Krok 5: Konfiguracja w PluginSettings

```rust
#[derive(Debug, Clone, Deserialize)]
pub struct PluginSettings {
    // ... istniejące pola
    #[serde(default)]
    pub myservice: PluginConfig,
}

impl PluginSettings {
    pub fn has_enabled_plugins(&self) -> bool {
        self.nginx.enabled || self.tomcat.enabled ||
        self.self_monitor.enabled || self.myservice.enabled
    }

    pub fn to_configs(&self) -> HashMap<String, PluginConfig> {
        // ... istniejący kod

        if self.myservice.enabled {
            let mut config = self.myservice.clone();
            if config.service_name.is_empty() {
                config.service_name = "myservice".to_string();
            }
            configs.insert("myservice".to_string(), config);
        }

        configs
    }
}
```

### Krok 6: Dodanie do config_manager.rs

Dodaj obsługę parametrów w `set_config_value()` i `VALID_CONFIG_KEYS`:

```rust
"plugins.myservice.enabled" => {
    let plugins = doc["plugins"].or_insert(toml_edit::table());
    let myservice = plugins["myservice"].or_insert(toml_edit::table());
    myservice["enabled"] = toml_edit::value(value.parse::<bool>().map_err(...)?);
}
```

### Krok 7: Testy

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_myservice_plugin_name() {
        let plugin = MyServicePlugin::new();
        assert_eq!(plugin.name(), "myservice");
    }

    #[test]
    fn test_myservice_patterns() {
        let plugin = MyServicePlugin::new();
        assert!(plugin.patterns.iter().any(|p| p.is_match("myservice")));
    }
}
```

---

## API i struktury danych

### Trait ServicePlugin

```rust
pub trait ServicePlugin: Send + Sync {
    /// Identyfikator pluginu (np. "nginx", "tomcat")
    fn name(&self) -> &'static str;

    /// Sprawdź czy usługa może być monitorowana
    fn is_available(&self, config: &PluginConfig) -> bool;

    /// Zbierz metryki usługi
    fn collect(
        &self,
        config: &PluginConfig,
        state: &mut PluginState,
        collect_processes: bool,
    ) -> io::Result<ServiceMetrics>;
}
```

### PluginState

Stan między cyklami zbierania dla obliczeń delta:

```rust
pub struct PluginState {
    pub prev_cpu_usec: u64,           // Poprzedni czas CPU
    pub prev_disk_read_bytes: u64,    // Poprzednie bajty odczytu
    pub prev_disk_write_bytes: u64,   // Poprzednie bajty zapisu
    pub prev_net_rx_bytes: u64,       // Poprzednie bajty rx
    pub prev_net_tx_bytes: u64,       // Poprzednie bajty tx
    pub last_collection_time: Option<Instant>,
    pub process_cpu_ticks: HashMap<u32, u64>,  // Per-process CPU
}
```

### DataSource

Enum określający źródło danych:

```rust
pub enum DataSource {
    Unknown,   // 0
    CgroupV2,  // 1 - preferowane
    CgroupV1,  // 2 - legacy
    ProcFs,    // 3 - fallback
}
```

### Protocol Buffers

Metryki są serializowane do Protocol Buffers i wysyłane przez MQTT:

```protobuf
message ServiceMetrics {
    string service_name = 1;
    string plugin_type = 2;
    bool is_running = 3;

    uint64 cpu_usage_usec = 10;
    double cpu_percent = 11;
    // ... pozostałe pola

    repeated ServiceProcessInfo processes = 70;
}

message ServiceMetricsList {
    google.protobuf.Timestamp timestamp = 1;
    string agent_id = 2;
    repeated ServiceMetrics services = 3;
}
```

---

## Dobre praktyki

1. **Preferuj cgroup** - daje agregowane metryki bez konieczności sumowania procesów
2. **Używaj regex ostrożnie** - kompiluj wzorce raz w konstruktorze
3. **Obsłuż błędy gracefully** - zwracaj `ServiceMetrics::not_running()` gdy usługa nie działa
4. **Zachowaj stan** - używaj `PluginState` do obliczeń delta (CPU%, I/O rate)
5. **Testuj** - dodaj testy jednostkowe dla wzorców i parsowania

---

## Rozwiązywanie problemów

### Plugin nie wykrywa usługi

1. Sprawdź czy serwis działa: `systemctl status nginx`
2. Sprawdź ścieżkę cgroup: `ls /sys/fs/cgroup/system.slice/nginx.service/`
3. Sprawdź czy nazwa w config pasuje: `service_name = "nginx"`

### Metryki CPU są 0%

- Pierwsze zbieranie zawsze zwraca 0% (brak poprzedniego stanu do delta)
- Sprawdź czy `plugin_interval_secs` nie jest za duży

### Brak metryk I/O z cgroup

- Niektóre kontenery/VM nie mają kontrolera io w cgroup
- Fallback do `/proc/[pid]/io` wymaga uprawnień root

### Plugin nie pojawia się w dashboardzie

1. Sprawdź czy `enabled = true` w settings.toml
2. Zrestartuj agenta po zmianie konfiguracji
3. Sprawdź logi agenta pod kątem błędów inicjalizacji pluginu
