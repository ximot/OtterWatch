# OtterWatch - Dokumentacja Techniczna

## Spis treści

1. [Opis systemu](#opis-systemu)
2. [Architektura ogólna](#architektura-ogólna)
3. [OtterWatch Agent](#otterwatch-agent)
4. [OtterWatch Server](#otterwatch-server)
5. [OtterWatch MQTT Broker](#otterwatch-mqtt-broker)
6. [Dashboard (Panel webowy)](#dashboard-panel-webowy)
7. [Protokół komunikacji](#protokół-komunikacji)
8. [System zdalnych poleceń](#system-zdalnych-poleceń)
9. [Schemat bazy danych](#schemat-bazy-danych)
10. [Instalacja i wdrożenie](#instalacja-i-wdrożenie)
11. [Konteneryzacja](#konteneryzacja)

---

## Opis systemu

**OtterWatch** to rozproszony system monitorowania wydajności serwerów Linux składający się z:

1. **Agent** - zbiera metryki systemowe i wysyła do serwera
2. **Server** - odbiera, przechowuje i udostępnia metryki
3. **Dashboard** - wizualizacja w panelu webowym

**Wersje:**
- Agent: 0.3.6
- Server: 0.3.3
- Dashboard: 0.3.0
- MQTT Broker: 0.1.0

**Licencja:** MIT
**Autor:** Tomasz Wyderka

---

## Architektura ogólna

```
┌─────────────────────────────────────────────────────────────────────────────┐
│                            SERWERY PRODUKCYJNE                              │
│                                                                             │
│  ┌──────────────┐  ┌──────────────┐  ┌──────────────┐  ┌──────────────┐   │
│  │   Agent 1    │  │   Agent 2    │  │   Agent 3    │  │   Agent N    │   │
│  │  (web-srv)   │  │   (db-srv)   │  │  (app-srv)   │  │     ...      │   │
│  │  group: web  │  │  group: db   │  │  group: ai   │  │     ...      │   │
│  │  /proc/*     │  │  /proc/*     │  │  /proc/*     │  │  /proc/*     │   │
│  │  CPU/MEM/IO  │  │  CPU/MEM/IO  │  │  CPU/MEM/IO  │  │  CPU/MEM/IO  │   │
│  └──────┬───────┘  └──────┬───────┘  └──────┬───────┘  └──────┬───────┘   │
│         │                 │                 │                 │           │
│         │    MQTT (Protocol Buffers) + Failover              │           │
│         └─────────────────┴─────────────────┴─────────────────┘           │
│                                   │                                        │
└───────────────────────────────────┼────────────────────────────────────────┘
                                    │
              ┌─────────────────────┼─────────────────────┐
              ▼                     ▼                     ▼
┌───────────────────────────────────────────────────────────────────────────┐
│                          INFRASTRUKTURA CENTRALNA                         │
│                                                                           │
│  ┌─────────────────────────────────────────────────────────────────────┐ │
│  │                    MQTT Broker Cluster (HA)                          │ │
│  │  ┌─────────────┐   ┌─────────────┐   ┌─────────────┐                │ │
│  │  │   Node 1    │◄─►│   Node 2    │◄─►│   Node 3    │   (bridge)     │ │
│  │  │  Port 1883  │   │  Port 1884  │   │  Port 1885  │                │ │
│  │  │ group: web  │   │ group: db   │   │ group: ai   │                │ │
│  │  └──────┬──────┘   └──────┬──────┘   └──────┬──────┘                │ │
│  └─────────┼─────────────────┼─────────────────┼───────────────────────┘ │
│            │                 │                 │                          │
│            └─────────────────┼─────────────────┘                          │
│                              ▼                                            │
│                   ┌─────────────────┐      ┌─────────────────┐           │
│                   │   OtterWatch    │      │   PostgreSQL/   │           │
│                   │     Server      │◄────►│   TimescaleDB   │           │
│                   │  (multi-sub)    │      │                 │           │
│                   │   Port: 8080    │      │   Port: 5432    │           │
│                   └────────┬────────┘      └─────────────────┘           │
│                            │                                              │
│                            │ REST API + Bootstrap                         │
│                            ▼                                              │
│                   ┌─────────────────┐                                    │
│                   │    Dashboard    │                                    │
│                   │  (React/Vite)   │                                    │
│                   │   Port: 5173    │                                    │
│                   └─────────────────┘                                    │
│                                                                           │
└───────────────────────────────────────────────────────────────────────────┘
```

### Kluczowe komponenty HA

| Komponent | Opis |
|-----------|------|
| **MQTT Broker Cluster** | Własny broker (otterwatch-mqtt) lub Mosquitto, możliwość klastra z bridgingiem |
| **Multi-Subscriber Server** | Serwer subskrybuje wiele brokerów jednocześnie |
| **Bootstrap Endpoint** | `GET /api/bootstrap?group=` - auto-discovery brokerów na podstawie grupy |
| **Agent Failover** | Automatyczne przełączanie między brokerami przy awarii |
| **SharedMqttPublisher** | Atomowa wymiana publishera przy failover (arc-swap) |

---

## OtterWatch Agent

### Przeznaczenie

Agent jest instalowany na każdym monitorowanym serwerze Linux. Jego zadaniem jest:

- Zbieranie metryk systemowych z `/proc/*`
- Wysyłanie danych do centralnego serwera przez MQTT
- Lokalne przechowywanie historii w plikach JSONL
- Udostępnianie lokalnego REST API
- Wykonywanie poleceń zdalnych (restart, aktualizacja, konfiguracja)
- Monitorowanie usług systemowych (pluginy)

### Moduły źródłowe

```
src/
├── main.rs              # Punkt wejścia, orkiestracja zadań async
├── app_config.rs        # Ładowanie konfiguracji (TOML + env)
├── agent_id.rs          # Zarządzanie UUID agenta
├── config_manager.rs    # Zarządzanie konfiguracją zdalną
├── bootstrap.rs         # HTTP bootstrap - auto-discovery brokerów
│
├── cpu.rs               # /proc/stat - użycie CPU, I/O wait
├── memory.rs            # /proc/meminfo - RAM, swap, procesy swap
├── disk_io.rs           # /proc/diskstats - I/O dysków
├── network.rs           # /proc/net/dev - statystyki sieci
├── pressure.rs          # /proc/pressure/ - PSI (Linux 4.20+)
├── process.rs           # /proc/[pid]/ - monitoring procesów
│
├── mqtt_client.rs       # Klient MQTT z failover i SharedMqttPublisher
├── message_queue.rs     # Kolejka offline na dysku
├── storage.rs           # Lokalne pliki JSONL
│
├── self_update.rs       # System auto-aktualizacji
├── console_ui.rs        # Interfejs konsolowy (crossterm)
├── osinfo.rs            # Informacje o systemie
│
├── plugins/             # System pluginów monitorowania usług
│   ├── mod.rs           # Registry pluginów
│   ├── cgroup.rs        # Zbieranie metryk z cgroups v1/v2
│   ├── nginx.rs         # Plugin nginx
│   ├── tomcat.rs        # Plugin tomcat
│   └── self_monitor.rs  # Plugin self-monitoring
│
└── proto/               # Protocol Buffers (wygenerowane)
    └── otterwatch.v1.rs
```

### Zadania asynchroniczne

| Zadanie | Interwał | Opis |
|---------|----------|------|
| `collect_and_save_stats` | 1s | Zbieranie metryk CPU, RAM, dysku, sieci |
| `collect_and_publish_processes` | 10s | Lista top N procesów |
| `collect_and_publish_services` | 10s | Metryki usług (pluginy) |
| `run_mqtt_event_loop` | ciągły | Obsługa połączenia MQTT |
| `handle_commands` | ciągły | Przetwarzanie poleceń z serwera |
| `clean_history_data` | 24h | Czyszczenie starych danych |
| HTTP Server | ciągły | REST API (Actix-web) |

### Zbierane metryki

| Metryka | Źródło | Częstotliwość |
|---------|--------|---------------|
| CPU usage (%) | `/proc/stat` | 1s |
| I/O wait (%) | `/proc/stat` | 1s |
| RAM used/available/total | `/proc/meminfo` | 1s |
| Swap free/total | `/proc/meminfo` | 1s |
| Disk I/O (read/write ops, time) | `/proc/diskstats` | 1s |
| Network (bytes rx/tx) | `/proc/net/dev` | 1s |
| PSI (avg10/60/300) | `/proc/pressure/` | 1s |
| Top N procesów | `/proc/[pid]/` | 10s |
| Metryki usług | cgroups/procfs | 10s |

### Konfiguracja agenta (settings.toml)

```toml
# Zbieranie danych
interval_secs = 1
process_list_interval_secs = 10
process_top_n = 25

# HTTP API lokalne
listen_addr = "127.0.0.1:8080"
cors_allowed_origins = "*"

# Przechowywanie lokalne
db_file_name = "system_stats.db"
db_save = true
db_history_days = 31
exclude_interfaces = "lo,wlan0"

# MQTT - pojedynczy broker
mqtt_enabled = true
mqtt_broker_addr = "tcp://server:1883"
mqtt_api_key = "secret-key"
mqtt_topic_prefix = "otterwatch/metrics"
mqtt_queue_path = "mqtt_queue"
mqtt_queue_max_size_mb = 100
mqtt_keepalive_secs = 30
mqtt_retry_interval_secs = 5

# MQTT - lista brokerów z failover (nadpisuje mqtt_broker_addr)
mqtt_broker_addrs = [
    "tcp://node1:1883",
    "tcp://node2:1883"
]

# Bootstrap URL - auto-discovery brokerów z serwera
mqtt_bootstrap_url = "http://server:8080/api/bootstrap"
mqtt_bootstrap_timeout_secs = 10

# Grupowanie (używane przez bootstrap do routingu)
agent_group = "web-servers"

# Pluginy monitorowania usług
[plugins]
plugin_interval_secs = 10
collect_process_details = true

[plugins.nginx]
enabled = true

[plugins.tomcat]
enabled = false

[plugins.self_monitor]
enabled = true
```

### Failover i Bootstrap

**Priorytet źródeł brokerów:**
1. `mqtt_bootstrap_url` - jeśli ustawiony, agent odpytuje serwer o listę brokerów
2. `mqtt_broker_addrs` - statyczna lista brokerów z failover
3. `mqtt_broker_addr` - pojedynczy broker (legacy)

**SharedMqttPublisher (arc-swap):**
- Po failover na inny broker, nowy publisher jest atomowo wymieniany
- Wszystkie taski automatycznie używają nowego połączenia
- Brak przerw w wysyłaniu metryk po przełączeniu

### Identyfikacja agenta

UUID agenta jest określany w następującej kolejności priorytetów:

1. **ID systemowe** (`/etc/otterwatch/agent_id`) - najwyższy priorytet, przetrwa reinstalację
2. **ID lokalne** (`{db_file_name}/agent_id`) - dla kompatybilności wstecznej
3. **ID z machine-id** - generowane deterministycznie z `/etc/machine-id`
4. **Losowy UUID** - fallback gdy brak machine-id

### Zależności agenta

| Biblioteka | Przeznaczenie |
|------------|---------------|
| tokio | Async runtime |
| actix-web | HTTP framework |
| rumqttc | Klient MQTT |
| prost | Protocol Buffers |
| crossterm | Terminal UI |
| reqwest | HTTP client (aktualizacje, bootstrap) |
| sha2 | Weryfikacja SHA256 |
| toml_edit | Edycja konfiguracji |
| arc-swap | Atomowa wymiana publishera przy failover |
| rand | Jitter dla reconnection (thundering herd prevention) |

---

## OtterWatch Server

### Przeznaczenie

Centralny serwer odpowiedzialny za:

- Odbieranie metryk od agentów przez MQTT
- Przechowywanie danych w PostgreSQL/TimescaleDB
- Udostępnianie REST API dla dashboardu
- Wysyłanie poleceń zdalnych do agentów
- Dystrybucję aktualizacji agentów
- Agregację metryk usług (pluginy)

### Moduły źródłowe

```
src/
├── main.rs              # Punkt wejścia, inicjalizacja
├── config/mod.rs        # Konfiguracja (TOML + env)
│
├── mqtt/
│   ├── mod.rs           # Re-eksporty
│   ├── subscriber.rs    # Event loop MQTT, routing wiadomości
│   └── auth.rs          # Walidacja kluczy API (SHA-256)
│
├── db/
│   ├── mod.rs           # Pool połączeń, migracje
│   ├── models.rs        # Modele: Agent, Metrics, Process, Service...
│   └── repository.rs    # Operacje CRUD (900+ linii)
│
├── api/
│   ├── mod.rs           # Re-eksporty
│   └── routes.rs        # Endpointy REST (Axum) - 35+ handlerów
│
├── updates/
│   └── mod.rs           # Zarządzanie wersjami aktualizacji
│
└── proto/               # Protocol Buffers (wygenerowane)
    └── otterwatch.v1.rs
```

### Konfiguracja serwera (settings.toml)

```toml
# HTTP API
http_listen_addr = "0.0.0.0:8080"

# MQTT - pojedynczy broker
mqtt_broker_addr = "tcp://localhost:1883"
mqtt_client_id = "otterwatch-server"
mqtt_topic_prefix = "otterwatch/metrics"

# MQTT - multi-broker (HA) - serwer subskrybuje wszystkie
mqtt_broker_addrs = [
    "tcp://node1:1883",
    "tcp://node2:1883",
    "tcp://node3:1883"
]

# Lista nodów MQTT dla bootstrap (publiczne adresy dla agentów)
mqtt_cluster_nodes = [
    "tcp://192.168.1.100:1883",
    "tcp://192.168.1.101:1883"
]

# Baza danych
database_url = "postgres://user:pass@localhost/otterwatch"

# Uwierzytelnianie
api_keys = "key1,key2,key3"

# Retencja danych
metrics_retention_days = 90

# Aktualizacje
updates_dir = "./updates"
public_url = "http://server-ip:8080"

# Bootstrap - routing agentów na podstawie grupy
[cluster_bootstrap]
enabled = true
fallback_broker = "tcp://192.168.1.100:1883"

[cluster_bootstrap.group_mapping]
web-servers = "tcp://192.168.1.100:1883"
databases = "tcp://192.168.1.101:1883"
ai = "tcp://192.168.1.102:1883"
```

Zmienne środowiskowe (prefix `OTTERWATCH_`):
- `OTTERWATCH_DATABASE_URL`
- `OTTERWATCH_HTTP_LISTEN_ADDR`
- `OTTERWATCH_MQTT_BROKER_ADDR`
- `OTTERWATCH_MQTT_BROKER_ADDRS` (JSON array)
- `OTTERWATCH_API_KEYS`
- `OTTERWATCH_UPDATES_DIR`
- `OTTERWATCH_PUBLIC_URL`

### REST API

#### Agenci

| Endpoint | Metoda | Opis |
|----------|--------|------|
| `/api/agents` | GET | Lista agentów (?group=filter) |
| `/api/agents/{id}` | GET | Szczegóły agenta |
| `/api/agents/{id}` | DELETE | Usuń agenta i metryki |
| `/api/agents/{id}/group` | PATCH | Zmień grupę agenta |
| `/api/agents/{id}/command` | POST | Wyślij polecenie |
| `/api/agents/{id}/command/{cmd_id}` | GET | Pobierz odpowiedź |

#### Metryki

| Endpoint | Metoda | Opis |
|----------|--------|------|
| `/api/agents/{id}/metrics?from=&to=` | GET | Metryki w zakresie |
| `/api/agents/{id}/metrics/latest` | GET | Ostatnie metryki |
| `/api/agents/{id}/metrics` | DELETE | Usuń metryki |
| `/api/agents/{id}/disk-metrics` | GET | Metryki dysków |
| `/api/agents/{id}/network-metrics` | GET | Metryki sieci |

#### Procesy

| Endpoint | Metoda | Opis |
|----------|--------|------|
| `/api/agents/{id}/processes` | GET | Historia procesów |
| `/api/agents/{id}/processes/latest` | GET | Ostatnia lista |
| `/api/agents/{id}/processes/at?at=` | GET | Procesy w momencie |

#### Usługi (Pluginy)

| Endpoint | Metoda | Opis |
|----------|--------|------|
| `/api/agents/{id}/services` | GET | Lista monitorowanych usług |
| `/api/agents/{id}/services/{name}/metrics` | GET | Historia metryk usługi |
| `/api/agents/{id}/services/{name}/metrics/latest` | GET | Ostatnie metryki usługi |
| `/api/agents/{id}/services/{name}/metrics` | DELETE | Usuń metryki usługi |
| `/api/agents/{id}/services/{name}/processes` | GET | Procesy usługi |

#### Grupy

| Endpoint | Metoda | Opis |
|----------|--------|------|
| `/api/groups` | GET | Lista unikalnych grup |

#### Serwer

| Endpoint | Metoda | Opis |
|----------|--------|------|
| `/health` | GET | Health check z wersją |
| `/api/version` | GET | Szczegóły wersji |
| `/api/server/stats` | GET | Statystyki serwera |

#### Bootstrap (Auto-Discovery)

| Endpoint | Metoda | Opis |
|----------|--------|------|
| `/api/bootstrap?group={group}` | GET | Konfiguracja MQTT dla agenta |

Odpowiedź bootstrap:
```json
{
  "primary_broker": "tcp://192.168.1.100:1883",
  "fallback_brokers": ["tcp://192.168.1.101:1883"],
  "topic_prefix": "otterwatch/metrics"
}
```

#### Aktualizacje

| Endpoint | Metoda | Opis |
|----------|--------|------|
| `/api/updates/latest` | GET | Najnowsza wersja |
| `/api/updates/versions` | GET | Lista wersji |
| `/api/updates/download/{ver}` | GET | Pobierz plik |
| `/api/updates/refresh` | POST | Odśwież listę |

### Zależności serwera

| Biblioteka | Przeznaczenie |
|------------|---------------|
| axum 0.7 | HTTP framework |
| sqlx 0.8 | PostgreSQL async driver |
| rumqttc 0.24 | Klient MQTT |
| prost 0.13 | Protocol Buffers |
| chrono 0.4 | Data/czas |
| uuid 1.0 | Identyfikacja agentów |
| sha2 0.10 | Checksumy SHA-256 |
| tower/tower-http | Middleware (CORS, tracing) |

---

## OtterWatch MQTT Broker

### Przeznaczenie

Własny broker MQTT zoptymalizowany dla systemu OtterWatch:

- Drop-in replacement dla Mosquitto
- Skalowalność do 10,000+ połączeń
- Wbudowane uwierzytelnianie API keys
- Metryki Prometheus
- Bridging między nodami dla HA
- Sharding agentów po grupach (planowane)

### Moduły źródłowe

```
src/
├── main.rs              # Entry point, CLI, signal handling
├── lib.rs               # Library exports
│
├── config/
│   ├── mod.rs           # Module exports
│   ├── schema.rs        # Configuration structs z defaults
│   └── loader.rs        # TOML + env loading, walidacja
│
├── auth/
│   ├── mod.rs           # Module exports
│   └── api_key.rs       # ApiKeyValidator (SHA256 + HashSet O(1))
│
├── broker/
│   ├── mod.rs           # Module exports
│   └── core.rs          # OtterWatchBroker, BrokerHandle, rumqttd wrapper
│
├── metrics/
│   └── mod.rs           # MetricsCollector, Prometheus counters
│
├── api/
│   └── mod.rs           # Axum routes: /health, /metrics, /api/stats
│
└── cluster/
    └── mod.rs           # HA coordination (planowane)
```

### Konfiguracja brokera (settings.toml)

```toml
[node]
name = "otterwatch-mqtt-1"
data_dir = "./data"

[mqtt]
listen_addr = "0.0.0.0:1883"
max_connections = 10000
router_buffer_size = 50000
keepalive_secs = 30
max_packet_size = 262144
max_qos = 1

[auth]
api_keys = ["your-api-key-here"]
require_auth = true
api_key_source = "password"

[metrics]
enabled = true
listen_addr = "0.0.0.0:9090"

[api]
listen_addr = "0.0.0.0:8085"
dashboard_enabled = true
cors_origins = ["*"]

# Bridge do innego brokera (opcjonalnie, dla HA)
[[bridge]]
name = "upstream"
addr = "192.168.1.100:1883"
topic = "otterwatch/#"
qos = 1
reconnection_delay_secs = 5
ping_delay_secs = 30
```

### API brokera

| Endpoint | Metoda | Opis |
|----------|--------|------|
| `/health` | GET | Health check z uptime |
| `/metrics` | GET | Metryki Prometheus |
| `/api/stats` | GET | Statystyki serwera (JSON) |
| `/api/connections` | GET | Lista połączeń (planowane) |

### Metryki Prometheus

```prometheus
otterwatch_mqtt_connections_active
otterwatch_mqtt_messages_received_total
otterwatch_mqtt_messages_sent_total
otterwatch_mqtt_bytes_received_total
otterwatch_mqtt_bytes_sent_total
```

### Zależności brokera

| Biblioteka | Przeznaczenie |
|------------|---------------|
| rumqttd 0.20 | MQTT broker library |
| tokio | Async runtime |
| axum | HTTP framework |
| prometheus | Metryki |
| dashmap | Concurrent HashMap |
| sha2 | API key hashing |
| tracing | Logging |

---

## Dashboard (Panel webowy)

### Przeznaczenie

Panel webowy do wizualizacji i zarządzania flotą serwerów:

- **Overview** - przegląd wszystkich agentów jako kafelki
- **Detail** - szczegóły pojedynczego agenta z wykresami
- **History** - przeglądanie historii metryk
- **Admin** - zarządzanie (usuwanie, polecenia, aktualizacje)

### Komponenty

```
dashboard/src/
├── App.tsx                      # Główny komponent z ViewSwitcher
├── main.tsx                     # Punkt wejścia
│
├── components/
│   ├── Header.tsx               # Nagłówek z nawigacją
│   ├── OverviewPanel.tsx        # Widok przeglądu floty
│   ├── DetailPanel.tsx          # Szczegóły pojedynczego agenta
│   ├── MetricsPanel.tsx         # Wykresy CPU/RAM w czasie rzeczywistym
│   ├── MetricsChart.tsx         # Komponent wykresu (Recharts)
│   ├── ProcessPanel.tsx         # Lista procesów
│   ├── AdminPanel.tsx           # Panel administracyjny
│   ├── HistoryPanel.tsx         # Panel historii
│   │
│   ├── AgentList.tsx            # Lista agentów z rozwijaniem
│   ├── AgentSelector.tsx        # Wybór agenta z wyszukiwaniem
│   ├── GaugeBar.tsx             # Wskaźnik użycia (pasek)
│   ├── TimeRangeSelector.tsx    # Wybór zakresu czasowego
│   │
│   ├── DiskIOChart.tsx          # Wykres I/O dysków
│   ├── NetworkChart.tsx         # Wykres sieci
│   ├── HistoryChart.tsx         # Wykres historyczny
│   ├── ProcessSnapshotViewer.tsx # Przeglądarka snapshotów procesów
│   └── ServiceMetricsPanel.tsx  # Panel metryk usług (pluginy)
│
├── hooks/
│   └── useApi.ts                # Hooki integracji z API
│
├── types/
│   └── api.ts                   # Definicje interfejsów TypeScript
│
└── utils/
    └── formatters.ts            # Funkcje formatowania
```

### Widoki (ViewSwitcher)

| Symbol | Widok | Opis |
|--------|-------|------|
| ▦ | OVERVIEW | Przegląd floty z kafelkami, filtrami, agregacją |
| ▤ | DETAIL | Szczegóły agenta z wykresami i procesami |
| ◷ | HISTORY | Historyczne metryki z zakresem dat |
| ⚙ | ADMIN | Panel zarządzania serwerem |

### Technologie

| Technologia | Wersja | Przeznaczenie |
|-------------|--------|---------------|
| React | 19.2 | Framework UI |
| TypeScript | 5.9 | Typowanie |
| Vite | 7.2 | Bundler z HMR |
| Tailwind CSS | 4.1 | Stylowanie utility-first |
| Recharts | 3.6 | Wykresy |
| date-fns | 4.1 | Operacje na datach |
| ESLint | 9.39 | Linting kodu |

### Funkcje panelu Admin

**Wskaźniki połączenia:**
- **RTT**: zielony (<50ms), żółty (<200ms), czerwony (>=200ms)
- **Stabilność**: STABLE (0 reconnect), OK (<5), UNSTABLE (<20), CRITICAL (>=20)

**Przyciski akcji:**
- **#** - symbol dla agentów z uprawnieniami root
- **CONFIG** - podgląd i edycja konfiguracji agenta (inline editing)
- **SWAP** - lista procesów używających swap
- **UPDATE** - aktualizacja pojedynczego agenta (semver porównanie)
- **UPDATE ALL** - masowa aktualizacja wszystkich agentów
- **REFRESH** - odświeżenie listy dostępnych wersji

**Edycja konfiguracji (CONFIG modal):**
- Kliknięcie na wartość zamienia ją w pole edycji
- Enter = zapis, Escape = anuluj
- Boolean: toggle przycisk (natychmiastowy zapis)
- Walidacja min/max dla liczb całkowitych
- Spinner podczas zapisywania, checkmark po sukcesie
- Sekcja Plugins do włączania/wyłączania pluginów (nginx, tomcat, self_monitor)

**Rozwijane wiersze agentów:**
- Kliknięcie rozwija wiersz ze wszystkimi szczegółami i przyciskami poleceń

**Status połączenia:**
- Footer: zielony "SYSTEM OPERATIONAL" lub czerwony "CONNECTION LOST"

### Hooki API (hooks/useApi.ts)

| Hook | Opis |
|------|------|
| `useAgents(refreshInterval)` | Auto-odświeżanie listy agentów |
| `useAgentMetrics(agentId)` | Metryki z buforem historii (60 punktów) |
| `useAllAgentsMetrics()` | Metryki wszystkich agentów |
| `useAgentProcesses()` | Top procesy |
| `useGroups()` | Dostępne grupy |
| `useServerHealth()` | Sprawdzenie połączenia z serwerem |
| `useAgentServices()` | Monitorowane usługi |
| `useAllServicesMetrics()` | Dane wszystkich usług |

---

## Protokół komunikacji

### Tematy MQTT

```
# Agent → Server (metryki)
{prefix}/{agent_id}/info        # Rejestracja (AgentInfo)
{prefix}/{agent_id}/snapshot    # Metryki (MetricsSnapshot)
{prefix}/{agent_id}/processes   # Procesy (ProcessList)
{prefix}/{agent_id}/services    # Usługi (ServiceMetricsList)
{prefix}/{agent_id}/status      # Status online/offline (LWT)

# Server → Agent (polecenia)
otterwatch/commands/{agent_id}/command    # Command

# Agent → Server (odpowiedzi)
otterwatch/responses/{agent_id}           # CommandResponse
```

### Wiadomości Protocol Buffers

#### AgentInfo (rejestracja)

```protobuf
message AgentInfo {
    string agent_id = 1;
    string hostname = 2;
    string os_name = 3;
    string kernel_version = 4;
    string agent_version = 5;
    uint32 cpu_cores = 6;
    string cpu_name = 7;
    string agent_group = 8;
    bool is_root = 9;
    uint32 queue_pending_count = 10;
    uint64 queue_pending_bytes = 11;
}
```

#### MetricsSnapshot

```protobuf
message MetricsSnapshot {
    google.protobuf.Timestamp timestamp = 1;
    string agent_id = 2;
    double cpu_usage_percent = 10;
    double cpu_io_wait_percent = 11;
    uint64 memory_used_kib = 20;
    uint64 memory_available_kib = 21;
    uint64 memory_total_kib = 22;
    uint64 swap_free_kib = 23;
    uint64 swap_total_kib = 24;
    repeated DiskMetrics disks = 30;
    repeated NetworkMetrics network = 40;
    optional PressureMetrics pressure = 50;
}
```

#### ProcessList

```protobuf
message ProcessList {
    google.protobuf.Timestamp timestamp = 1;
    string agent_id = 2;
    repeated ProcessInfo processes = 3;
}

message ProcessInfo {
    uint32 pid = 1;
    string name = 2;
    string state = 3;
    uint32 ppid = 4;
    double cpu_percent = 5;
    uint64 memory_rss_kib = 6;
    uint64 memory_vsz_kib = 7;
    uint64 threads = 8;
    string user = 9;
    string cmdline = 10;
}
```

#### ServiceMetrics (pluginy)

```protobuf
message ServiceMetrics {
    string service_name = 1;
    string plugin_type = 2;
    bool is_running = 3;

    // CPU
    uint64 cpu_usage_usec = 10;
    double cpu_percent = 11;
    uint64 cpu_user_usec = 12;
    uint64 cpu_system_usec = 13;

    // Memory
    uint64 memory_current_bytes = 20;
    uint64 memory_swap_bytes = 21;
    uint64 memory_anon_bytes = 22;
    uint64 memory_file_bytes = 23;

    // Disk I/O
    uint64 disk_read_bytes = 30;
    uint64 disk_write_bytes = 31;
    uint64 disk_read_ops = 32;
    uint64 disk_write_ops = 33;

    // Network I/O
    uint64 net_rx_bytes = 40;
    uint64 net_tx_bytes = 41;

    // Process info
    uint32 process_count = 50;
    uint32 thread_count = 51;

    // Metadata
    uint32 cgroup_version = 60;
    uint32 data_source = 61;  // 0=unknown, 1=cgroupv2, 2=cgroupv1, 3=procfs

    repeated ServiceProcessInfo processes = 70;
}
```

---

## System zdalnych poleceń

### Dostępne polecenia

| Polecenie | Kod wyjścia | Opis |
|-----------|-------------|------|
| `ping` | - | Health check, odpowiada "pong", mierzony RTT |
| `reload-config` | - | Walidacja settings.toml |
| `restart` | 42 | Restart przez systemd/wrapper |
| `reconnect` | 44 | Re-fetch bootstrap i reconnect do nowego brokera |
| `set-group` | - | Zmiana grupy agenta (triggeruje też reconnect) |
| `update` | 43 | Pobieranie i instalacja aktualizacji |
| `get-config` | - | Zwraca konfigurację agenta |
| `get-swap-processes` | - | Lista procesów używających swap |
| `set-config` | - | Ustaw pojedynczy parametr konfiguracji |
| `sync-config` | - | Dodaj brakujące parametry konfiguracji |
| `get-config-schema` | - | Pobierz pełny schemat konfiguracji |

### Kody wyjścia

| Kod | Znaczenie | Akcja wrappera |
|-----|-----------|----------------|
| 0 | Normalne zakończenie | Restart po 2s |
| 42 | Żądanie restartu | Restart natychmiast |
| 43 | Aktualizacja gotowa | Zamiana binary, restart |
| 44 | Reconnect | Restart natychmiast (re-bootstrap) |

### Przepływ aktualizacji

```
┌────────────┐      ┌────────────┐      ┌────────────┐
│  Dashboard │      │   Server   │      │   Agent    │
└─────┬──────┘      └─────┬──────┘      └─────┬──────┘
      │                   │                   │
      │  POST /command    │                   │
      │  (update)         │                   │
      │──────────────────►│                   │
      │                   │  MQTT: Command    │
      │                   │  (URL, SHA256)    │
      │                   │──────────────────►│
      │                   │                   │
      │                   │                   │ 1. Download binary
      │                   │                   │ 2. Verify SHA256
      │                   │                   │ 3. Backup current
      │                   │                   │ 4. Write .new file
      │                   │                   │
      │                   │  CommandResponse  │
      │                   │◄──────────────────│
      │                   │                   │
      │                   │                   │ 5. Exit code 43
      │                   │                   │
      │                   │                   │ [Wrapper Script]
      │                   │                   │ 6. Replace binary
      │                   │                   │ 7. Health check
      │                   │                   │ 8. Rollback if fail
```

### Struktura katalogu aktualizacji

```
updates/
├── otterwatch-0.1.0
├── otterwatch-0.1.1
├── ...
├── otterwatch-0.2.0
├── otterwatch-0.2.1
└── otterwatch-0.2.3
```

---

## Schemat bazy danych

### Tabele PostgreSQL/TimescaleDB

```sql
-- Agenci
CREATE TABLE agents (
    id UUID PRIMARY KEY,
    hostname VARCHAR(255) NOT NULL,
    os_name VARCHAR(255) NOT NULL,
    kernel_version VARCHAR(100) NOT NULL,
    agent_version VARCHAR(50) NOT NULL,
    cpu_cores INTEGER NOT NULL,
    cpu_name VARCHAR(255) NOT NULL,
    api_key_hash VARCHAR(64) NOT NULL,
    created_at TIMESTAMPTZ DEFAULT NOW(),
    last_seen_at TIMESTAMPTZ,
    is_online BOOLEAN DEFAULT false,
    agent_group VARCHAR(100),
    reconnect_count INTEGER DEFAULT 0,
    last_disconnect_at TIMESTAMPTZ,
    last_rtt_ms INTEGER,
    last_rtt_at TIMESTAMPTZ,
    is_root BOOLEAN DEFAULT false,
    queue_pending_count INTEGER DEFAULT 0,
    queue_pending_bytes BIGINT DEFAULT 0
);

-- Metryki główne (hypertable)
CREATE TABLE metrics (
    time TIMESTAMPTZ NOT NULL,
    agent_id UUID REFERENCES agents(id) ON DELETE CASCADE,
    cpu_usage DOUBLE PRECISION NOT NULL,
    cpu_io_wait DOUBLE PRECISION NOT NULL,
    memory_used_kib BIGINT NOT NULL,
    memory_available_kib BIGINT NOT NULL,
    memory_total_kib BIGINT NOT NULL,
    swap_free_kib BIGINT NOT NULL,
    swap_total_kib BIGINT NOT NULL,
    PRIMARY KEY (time, agent_id)
);

-- Metryki dysków (hypertable)
CREATE TABLE disk_metrics (
    time TIMESTAMPTZ NOT NULL,
    agent_id UUID REFERENCES agents(id) ON DELETE CASCADE,
    device VARCHAR(50) NOT NULL,
    read_ops BIGINT NOT NULL,
    write_ops BIGINT NOT NULL,
    read_time_ms BIGINT NOT NULL,
    write_time_ms BIGINT NOT NULL,
    PRIMARY KEY (time, agent_id, device)
);

-- Metryki sieci (hypertable)
CREATE TABLE network_metrics (
    time TIMESTAMPTZ NOT NULL,
    agent_id UUID REFERENCES agents(id) ON DELETE CASCADE,
    interface_name VARCHAR(50) NOT NULL,
    bytes_received BIGINT NOT NULL,
    bytes_transmitted BIGINT NOT NULL,
    PRIMARY KEY (time, agent_id, interface_name)
);

-- Snapshoty procesów
CREATE TABLE process_snapshots (
    time TIMESTAMPTZ NOT NULL,
    agent_id UUID REFERENCES agents(id) ON DELETE CASCADE,
    pid INTEGER NOT NULL,
    name VARCHAR(255) NOT NULL,
    state CHAR(1) NOT NULL,
    ppid INTEGER NOT NULL,
    cpu_percent DOUBLE PRECISION NOT NULL,
    memory_rss_kib BIGINT NOT NULL,
    memory_vsz_kib BIGINT NOT NULL,
    threads INTEGER NOT NULL,
    username VARCHAR(100),
    cmdline TEXT,
    start_time BIGINT,
    PRIMARY KEY (time, agent_id, pid)
);

-- Metryki usług (hypertable)
CREATE TABLE service_metrics (
    time TIMESTAMPTZ NOT NULL,
    agent_id UUID REFERENCES agents(id) ON DELETE CASCADE,
    service_name VARCHAR(100) NOT NULL,
    plugin_type VARCHAR(50) NOT NULL,
    is_running BOOLEAN NOT NULL,

    -- CPU
    cpu_usage_usec BIGINT,
    cpu_percent DOUBLE PRECISION,
    cpu_user_usec BIGINT,
    cpu_system_usec BIGINT,

    -- Memory
    memory_current_bytes BIGINT,
    memory_swap_bytes BIGINT,
    memory_anon_bytes BIGINT,
    memory_file_bytes BIGINT,

    -- Disk I/O
    disk_read_bytes BIGINT,
    disk_write_bytes BIGINT,
    disk_read_ops BIGINT,
    disk_write_ops BIGINT,

    -- Network I/O
    net_rx_bytes BIGINT,
    net_tx_bytes BIGINT,

    -- Process info
    process_count INTEGER,
    thread_count INTEGER,

    -- Metadata
    cgroup_version INTEGER,
    data_source INTEGER,

    PRIMARY KEY (time, agent_id, service_name)
);

-- Procesy usług
CREATE TABLE service_processes (
    time TIMESTAMPTZ NOT NULL,
    agent_id UUID REFERENCES agents(id) ON DELETE CASCADE,
    service_name VARCHAR(100) NOT NULL,
    pid INTEGER NOT NULL,
    name VARCHAR(255) NOT NULL,
    cpu_percent DOUBLE PRECISION NOT NULL,
    memory_bytes BIGINT NOT NULL,
    threads INTEGER NOT NULL,
    PRIMARY KEY (time, agent_id, service_name, pid)
);

-- Cache odpowiedzi poleceń
CREATE TABLE command_responses (
    command_id VARCHAR(36) PRIMARY KEY,
    agent_id UUID REFERENCES agents(id) ON DELETE CASCADE,
    success BOOLEAN NOT NULL,
    message TEXT NOT NULL,
    config_json TEXT,
    swap_processes_json TEXT,
    created_at TIMESTAMPTZ DEFAULT NOW()
);
```

### Indeksy i hypertables

Wszystkie tabele metryk wspierają konwersję do hypertables TimescaleDB:

```sql
-- TimescaleDB hypertables
SELECT create_hypertable('metrics', 'time');
SELECT create_hypertable('disk_metrics', 'time');
SELECT create_hypertable('network_metrics', 'time');
SELECT create_hypertable('process_snapshots', 'time');
SELECT create_hypertable('service_metrics', 'time');
SELECT create_hypertable('service_processes', 'time');

-- Retencja: 90 dni (domyślnie)
SELECT add_retention_policy('metrics', INTERVAL '90 days');
```

---

## Instalacja i wdrożenie

### Wymagania

- **Linux** (agent działa tylko na Linux)
- **PostgreSQL** 14+ z TimescaleDB (serwer)
- **MQTT Broker** (np. Mosquitto)
- **Node.js** 18+ (dashboard)
- **Rust** 1.68.2+ (kompilacja)

### Instalacja agenta

```bash
# Kompilacja
cd otterwatch
cargo build --release

# Lub pełny pakiet
./scripts/build-release.sh

# Instalacja
sudo cp dist/otterwatch /usr/bin/
sudo cp dist/otterwatch-wrapper.sh /usr/bin/
sudo cp dist/otterwatch.service /etc/systemd/system/

# Konfiguracja
sudo mkdir -p /etc/otterwatch
sudo cp dist/settings.toml.example /etc/otterwatch/settings.toml
sudo nano /etc/otterwatch/settings.toml

# Start
sudo systemctl daemon-reload
sudo systemctl enable otterwatch
sudo systemctl start otterwatch
```

### Instalacja serwera

```bash
# Kompilacja
cd otterwatch-server
cargo build --release

# Przygotowanie bazy
createdb otterwatch
sqlx migrate run

# Konfiguracja
cp settings.toml.example settings.toml
nano settings.toml

# Start
./target/release/otterwatch-server
```

### Instalacja dashboardu

```bash
cd otterwatch-server/dashboard

# Instalacja zależności
npm install

# Development
npm run dev

# Production build
npm run build

# Preview production
npm run preview
```

---

## Konteneryzacja

### Docker/Podman

Projekt zawiera pliki do konteneryzacji w `otterwatch-server/container/`:

```
container/
├── README.md           # Przewodnik wdrożenia
├── mosquitto.conf      # Konfiguracja MQTT broker
├── init-db.sql         # Inicjalizacja bazy danych
└── start.sh            # Skrypt pomocniczy
```

### Struktura podman-compose.yml

```yaml
services:
  db:
    image: timescale/timescaledb:latest-pg16
    ports:
      - "5432:5432"
    volumes:
      - otterwatch-db-data:/var/lib/postgresql/data
    healthcheck: ...

  mqtt:
    image: eclipse-mosquitto:2
    ports:
      - "1883:1883"   # MQTT
      - "9001:9001"   # WebSocket
    volumes:
      - otterwatch-mqtt-data:/mosquitto/data
      - otterwatch-mqtt-log:/mosquitto/log
    healthcheck: ...

  server:
    build: .
    ports:
      - "8080:8080"
    volumes:
      - otterwatch-updates:/app/updates
    depends_on:
      - db
      - mqtt
    healthcheck: ...
```

### Szybki start

```bash
# Kopiuj środowisko
cp .env.example .env

# Edytuj .env z kluczami API i publicznym URL
OTTERWATCH_API_KEYS=your-secret
OTTERWATCH_PUBLIC_URL=http://server-ip:8080

# Start stack
podman-compose up -d
# lub użyj skryptu pomocniczego
./container/start.sh up

# Dashboard dostępny pod http://localhost:8080
```

### Multi-stage Dockerfile

1. **Builder stage** - Rust latest, buduje server binary, kompiluje protobuf
2. **Dashboard stage** - Node 20, buduje React app
3. **Runtime stage** - Debian bookworm-slim, minimalna wielkość

---

## Lokalizacje projektów

| Komponent | Ścieżka |
|-----------|---------|
| Agent | `/home/ximot/RustroverProjects/monitoring/otterwatch` |
| Server | `/home/ximot/RustroverProjects/monitoring/otterwatch-server` |
| MQTT Broker | `/home/ximot/RustroverProjects/monitoring/otterwatch-mqtt` |
| Dashboard | `/home/ximot/RustroverProjects/monitoring/otterwatch-server/dashboard` |
| Proto | `/home/ximot/RustroverProjects/monitoring/otterwatch-server/proto` |
| Migracje | `/home/ximot/RustroverProjects/monitoring/otterwatch-server/migrations` |

---

## Komendy deweloperskie

### Agent (Rust)

```bash
cargo build --release    # Build release
cargo build              # Build debug
cargo run                # Uruchom
cargo run -- --gui       # Uruchom z UI konsolowym
cargo test               # Testy
cargo check              # Sprawdź kompilację
cargo fmt                # Formatowanie
cargo clippy             # Linter
./scripts/build-release.sh  # Pełny pakiet release
```

### Server (Rust)

```bash
cargo build --release    # Build release
cargo build              # Build debug
cargo run                # Uruchom
cargo test               # Testy
cargo check              # Sprawdź kompilację
cargo fmt                # Formatowanie
cargo clippy             # Linter
sqlx migrate run         # Migracje bazy
```

### MQTT Broker (Rust)

```bash
cargo build --release    # Build release
cargo build              # Build debug
cargo run                # Uruchom broker
cargo run -- -c /path/to/settings.toml  # Własna konfiguracja
cargo test               # Testy
cargo check              # Sprawdź kompilację
cargo fmt                # Formatowanie
cargo clippy             # Linter
```

### Dashboard (React/Vite)

```bash
npm install              # Instalacja zależności
npm run dev              # Development server (HMR)
npm run build            # Production build
npm run preview          # Preview production
npm run lint             # Linting
```
