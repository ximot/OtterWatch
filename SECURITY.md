# OtterWatch - Dokumentacja Bezpieczeństwa

Dokument przeznaczony dla jednostki bezpieczeństwa opisujący architekturę zabezpieczeń, model zagrożeń, oraz rekomendacje hardening dla systemu OtterWatch.

**Wersja:** 0.3.6
**Data:** 2024-12

---

## Spis treści

1. [Przegląd systemu](#przegląd-systemu)
2. [Model zagrożeń](#model-zagrożeń)
3. [Architektura bezpieczeństwa](#architektura-bezpieczeństwa)
4. [Uwierzytelnianie](#uwierzytelnianie)
5. [Komunikacja sieciowa](#komunikacja-sieciowa)
6. [Ochrona danych](#ochrona-danych)
7. [Logowanie i audyt](#logowanie-i-audyt)
8. [Zarządzanie sekretami](#zarządzanie-sekretami)
9. [Hardening produkcyjny](#hardening-produkcyjny)
10. [Znane ograniczenia](#znane-ograniczenia)

---

## Przegląd systemu

OtterWatch to rozproszony system monitorowania wydajności serwerów Linux składający się z:

| Komponent | Rola | Porty |
|-----------|------|-------|
| **Agent** | Zbiera metryki z /proc/*, wysyła do serwera | 8080 (HTTP lokalny) |
| **Server** | Agreguje metryki, API REST, dashboard | 8080 (HTTP) |
| **MQTT Broker** | Routing wiadomości między agentami a serwerem | 1883 (MQTT), 9090 (metrics) |
| **PostgreSQL** | Przechowywanie metryk | 5432 |
| **Dashboard** | Panel webowy (React) | 5173 (dev) / 8080 (prod) |

### Przepływ danych

```
Agent → [MQTT/TLS] → MQTT Broker → [MQTT/TLS] → Server → [SQL] → PostgreSQL
                                                    ↓
                                            Dashboard (HTTP)
```

---

## Model zagrożeń

### Aktorzy zagrożeń

| Aktor | Poziom | Wektor ataku |
|-------|--------|--------------|
| Zewnętrzny atakujący | Niski | Skanowanie portów, MITM |
| Skompromitowany agent | Średni | Fałszywe metryki, DoS |
| Nieuprawniony admin | Średni | Nadużycie poleceń zdalnych |
| Atakujący wewnętrzny | Wysoki | Przechwytywanie API keys |

### Zagrożenia i mitygacje

| ID | Zagrożenie | Ryzyko | Mitygacja |
|----|------------|--------|-----------|
| T1 | Przechwycenie API key w transmisji | Wysokie | TLS dla MQTT i HTTP |
| T2 | Nieautoryzowany dostęp do API | Wysokie | Walidacja API keys |
| T3 | Wstrzyknięcie fałszywych metryk | Średnie | Walidacja agent_id + api_key_hash |
| T4 | Zdalne wykonanie kodu przez update | Wysokie | SHA256 checksum, wrapper rollback |
| T5 | DoS przez queue flooding | Średnie | Limit rozmiaru kolejki (mqtt_queue_max_size_mb) |
| T6 | Eskalacja uprawnień | Niskie | Agent może działać bez root |
| T7 | Wyciek danych przez procesy | Niskie | Tylko metadane procesów, bez argumentów wrażliwych |

---

## Architektura bezpieczeństwa

### Warstwy zabezpieczeń

```
┌─────────────────────────────────────────────────────────────┐
│                    WARSTWA TRANSPORTOWA                      │
│  TLS 1.2+/1.3 dla MQTT (opcjonalnie) i HTTPS                │
├─────────────────────────────────────────────────────────────┤
│                    WARSTWA UWIERZYTELNIANIA                  │
│  API Keys (SHA256 hash), Agent ID (UUID)                     │
├─────────────────────────────────────────────────────────────┤
│                    WARSTWA AUTORYZACJI                       │
│  Grupowanie agentów, polecenia wymagają online status       │
├─────────────────────────────────────────────────────────────┤
│                    WARSTWA INTEGRALNOŚCI                     │
│  Protocol Buffers (binarne), SHA256 dla aktualizacji        │
├─────────────────────────────────────────────────────────────┤
│                    WARSTWA DOSTĘPNOŚCI                       │
│  Failover MQTT, offline queue, multi-broker                  │
└─────────────────────────────────────────────────────────────┘
```

### Granice zaufania

1. **Agent ↔ MQTT Broker** - wymaga api_key w polu MQTT password
2. **MQTT Broker ↔ Server** - wymaga api_key w wiadomości protobuf
3. **Dashboard ↔ Server** - wymaga aktywnej sesji (obecnie brak auth)
4. **Server ↔ PostgreSQL** - wymaga hasła w connection string

---

## Uwierzytelnianie

### API Keys

**Generowanie:**
```bash
# Zalecane: 32+ znaki, losowe
openssl rand -base64 32
```

**Przechowywanie:**
- Agent: `settings.toml` → `mqtt_api_key` lub env `APP_MQTT_API_KEY`
- Server: `settings.toml` → `api_keys` lub env `OTTERWATCH_API_KEYS`
- MQTT Broker: `settings.toml` → `auth.api_keys`

**Haszowanie:**
- Klucze przechowywane w bazie jako SHA256 hash
- Porównanie w czasie stałym (constant-time)
- Lookup O(1) przez HashSet

**Walidacja:**
```
Agent wysyła:
  MQTT password = api_key
  Protobuf.AuthenticatedMessage.api_key = api_key

Server sprawdza:
  SHA256(api_key) == agents.api_key_hash
```

### Agent ID (UUID)

**Źródła (priorytet):**
1. `/etc/otterwatch/agent_id` - przetrwa reinstalację
2. `{db_path}/agent_id` - lokalny
3. Deterministyczny z `/etc/machine-id`
4. Losowy UUID v4 (fallback)

**Zabezpieczenia:**
- Nie można zmienić agent_id po rejestracji
- Konflikt agent_id → odrzucenie (różny api_key_hash)

---

## Komunikacja sieciowa

### MQTT (Agent ↔ Broker)

| Parametr | Wartość domyślna | Zalecana produkcja |
|----------|------------------|--------------------|
| Port | 1883 (TCP) | 8883 (TLS) |
| TLS | wyłączony | TLS 1.2+ wymagany |
| Keepalive | 30s | 30-60s |
| Clean session | true | true |
| QoS | 1 (at least once) | 1 |

**Konfiguracja TLS (agent):**
```toml
mqtt_broker_addr = "ssl://broker:8883"
# Certyfikaty przez system trust store
```

**Konfiguracja TLS (broker):**
```toml
[mqtt]
tls_cert = "/etc/otterwatch/cert.pem"
tls_key = "/etc/otterwatch/key.pem"
```

### HTTP (Server API)

| Parametr | Wartość domyślna | Zalecana produkcja |
|----------|------------------|--------------------|
| Port | 8080 | 443 (za reverse proxy) |
| CORS | * (wszystkie) | lista dozwolonych origin |
| Rate limiting | brak | zalecany na proxy |

**Przykład nginx reverse proxy:**
```nginx
server {
    listen 443 ssl;
    ssl_certificate /etc/ssl/otterwatch.crt;
    ssl_certificate_key /etc/ssl/otterwatch.key;

    location / {
        proxy_pass http://127.0.0.1:8080;
        limit_req zone=api burst=20;
    }
}
```

### Firewall

**Minimalne reguły:**
```bash
# Agent → MQTT Broker
iptables -A OUTPUT -p tcp --dport 1883 -j ACCEPT
iptables -A OUTPUT -p tcp --dport 8883 -j ACCEPT  # TLS

# Server → PostgreSQL
iptables -A OUTPUT -p tcp --dport 5432 -j ACCEPT

# Dashboard → Server (przez proxy)
iptables -A INPUT -p tcp --dport 443 -j ACCEPT
```

---

## Ochrona danych

### Dane zbierane przez agenta

| Kategoria | Dane | Wrażliwość |
|-----------|------|------------|
| System | hostname, kernel, CPU model | Niska |
| Metryki | CPU%, RAM, I/O, sieć | Niska |
| Procesy | PID, nazwa, user, cmdline | Średnia* |
| Swap | PID, nazwa, swap KB | Średnia* |

*cmdline może zawierać argumenty wrażliwe (hasła w CLI)

**Mitygacja:**
- Nie loguj pełnego cmdline w production
- Ogranicz `process_top_n` do minimum
- Agent bez root widzi tylko własne procesy

### Dane w PostgreSQL

| Tabela | Retencja | Sensytywność |
|--------|----------|--------------|
| agents | permanentna | Niska (api_key_hash) |
| metrics | konfigurowalna | Niska |
| process_snapshots | konfigurowalna | Średnia |
| command_responses | 1h | Niska |

**Polityka retencji:**
```sql
-- TimescaleDB automatyczna retencja
SELECT add_retention_policy('metrics', INTERVAL '90 days');
SELECT add_retention_policy('process_snapshots', INTERVAL '7 days');
```

### Szyfrowanie at-rest

- PostgreSQL: włącz szyfrowanie dysku (LUKS/dm-crypt)
- Agent queue: pliki JSONL, bez szyfrowania (tylko metryki)
- Logi: rozważ szyfrowanie dla cmdline

---

## Logowanie i audyt

### Poziomy logowania

| Komponent | Poziom domyślny | Produkcja |
|-----------|-----------------|-----------|
| Agent | info | warn |
| Server | info | info |
| MQTT Broker | info | warn |

**Konfiguracja:**
```bash
RUST_LOG=warn cargo run          # Agent/Server
RUST_LOG=otterwatch=debug,warn   # Szczegółowe tylko dla otterwatch
```

### Zdarzenia audytowane

| Zdarzenie | Poziom | Dane |
|-----------|--------|------|
| Agent connect | INFO | agent_id, broker |
| Agent disconnect | INFO | agent_id, reason |
| Command received | INFO | agent_id, command_type, command_id |
| Command executed | INFO | agent_id, command_type, success |
| Update download | INFO | agent_id, version, checksum |
| API key mismatch | WARN | agent_id, expected_hash |
| Failover | INFO | old_broker, new_broker |

### Format logów

```
[2024-12-20T10:30:15Z INFO  otterwatch::mqtt_client] Connected to MQTT broker: tcp://192.168.1.100:1883
[2024-12-20T10:30:16Z INFO  otterwatch] Received command: Ping (id: 550e8400-e29b-41d4-a716-446655440000)
[2024-12-20T10:30:16Z INFO  otterwatch] Processing command: Ping (id: 550e8400-e29b-41d4-a716-446655440000)
```

### Integracja z SIEM

- Format: JSON lines (przez `env_logger` z custom formatter)
- Transport: syslog, journald, lub file → fluentd/filebeat

---

## Zarządzanie sekretami

### Wrażliwe parametry

| Parametr | Lokalizacja | Ochrona |
|----------|-------------|---------|
| `mqtt_api_key` | Agent settings.toml | chmod 600, właściciel root |
| `api_keys` | Server settings.toml | chmod 600, właściciel root |
| `database_url` | Server settings.toml | chmod 600, hasło w URL |
| `auth.api_keys` | Broker settings.toml | chmod 600 |

### Zmienne środowiskowe (zalecane)

```bash
# Agent
export APP_MQTT_API_KEY="your-secret-key"

# Server
export OTTERWATCH_API_KEYS="key1,key2,key3"
export OTTERWATCH_DATABASE_URL="postgres://user:pass@localhost/db"

# Broker
export OTTERWATCH_MQTT_AUTH__API_KEYS="key1,key2"
```

### Rotacja kluczy

1. Dodaj nowy klucz do `api_keys` serwera
2. Zaktualizuj agentów (set-config lub update)
3. Usuń stary klucz po migracji wszystkich agentów

**Polecenie rotacji:**
```bash
# Z dashboardu lub API
curl -X POST http://server:8080/api/agents/{id}/command \
  -H "Content-Type: application/json" \
  -d '{"command": "set-config", "config_key": "mqtt_api_key", "config_value": "new-key"}'
```

---

## Hardening produkcyjny

### Agent

```bash
# Uruchom jako dedykowany user (nie root)
useradd -r -s /sbin/nologin otterwatch
chown otterwatch:otterwatch /usr/bin/otterwatch
chown -R otterwatch:otterwatch /etc/otterwatch
chmod 700 /etc/otterwatch
chmod 600 /etc/otterwatch/settings.toml

# Systemd hardening
[Service]
User=otterwatch
Group=otterwatch
NoNewPrivileges=yes
ProtectSystem=strict
ProtectHome=yes
PrivateTmp=yes
ReadOnlyPaths=/
ReadWritePaths=/var/lib/otterwatch
CapabilityBoundingSet=
```

**Uwaga:** Agent jako non-root nie widzi procesów innych użytkowników. Dla pełnego monitorowania wymagany root lub capability CAP_SYS_PTRACE.

### Server

```bash
# Dedykowany user
useradd -r -s /sbin/nologin otterwatch-server

# Ograniczenie portów
[Service]
User=otterwatch-server
AmbientCapabilities=CAP_NET_BIND_SERVICE  # jeśli port < 1024
```

### MQTT Broker

```bash
# Dedykowany user
useradd -r -s /sbin/nologin otterwatch-mqtt

# Limit połączeń
[mqtt]
max_connections = 10000  # dostosuj do potrzeb
```

### PostgreSQL

```sql
-- Minimalny user dla serwera
CREATE USER otterwatch WITH PASSWORD 'strong-password';
GRANT CONNECT ON DATABASE otterwatch TO otterwatch;
GRANT USAGE ON SCHEMA public TO otterwatch;
GRANT SELECT, INSERT, UPDATE, DELETE ON ALL TABLES IN SCHEMA public TO otterwatch;
GRANT USAGE ON ALL SEQUENCES IN SCHEMA public TO otterwatch;

-- Nie dawaj SUPERUSER, CREATEDB, CREATEROLE
```

### Checklist produkcyjny

- [ ] TLS dla MQTT (port 8883)
- [ ] HTTPS dla dashboard (nginx/caddy)
- [ ] Firewall ograniczający porty
- [ ] API keys z >32 znaków
- [ ] Dedykowane konta systemowe
- [ ] Systemd hardening
- [ ] Log rotation
- [ ] Monitoring dostępności (Prometheus/Alertmanager)
- [ ] Backup PostgreSQL
- [ ] Retencja danych skonfigurowana
- [ ] CORS ograniczony do zaufanych domen

---

## Znane ograniczenia

### Brak uwierzytelniania dashboardu

**Status:** Dashboard nie wymaga logowania
**Ryzyko:** Każdy z dostępem sieciowym może przeglądać i zarządzać agentami
**Mitygacja:**
- Ogranicz dostęp przez firewall/VPN
- Reverse proxy z basic auth lub SSO

### Brak szyfrowania offline queue

**Status:** Kolejka offline przechowuje metryki jako plain JSON
**Ryzyko:** Lokalny dostęp pozwala odczytać historię metryk
**Mitygacja:**
- Ograniczenie uprawnień do katalogu queue
- Szyfrowanie dysku (LUKS)

### Polecenia bez dodatkowej autoryzacji

**Status:** Każdy klucz API może wysyłać dowolne polecenia
**Ryzyko:** Skompromitowany klucz pozwala na restart/update
**Mitygacja:**
- Rozdzielenie kluczy read-only vs admin
- Logowanie wszystkich poleceń

### TLS opcjonalny

**Status:** TLS dla MQTT wymaga ręcznej konfiguracji
**Ryzyko:** Transmisja plain text w niezaufanych sieciach
**Mitygacja:**
- Zawsze włączaj TLS w produkcji
- VPN dla komunikacji agent ↔ broker

---

## Kontakt

**Zgłaszanie podatności:** security@example.com
**PGP Key:** [link do klucza publicznego]

Prosimy o odpowiedzialne ujawnianie (responsible disclosure) z 90-dniowym embargo przed publikacją.
