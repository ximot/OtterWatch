# OtterWatch Docker Build Scripts

Kompletny zestaw skryptów do budowania obrazów Docker dla całego ekosystemu OtterWatch.

## Zawartość

- `build-all-images.sh` - Główny skrypt budujący wszystkie obrazy
- `build-mqtt.sh` - Buduje tylko obraz MQTT brokera
- `build-server.sh` - Buduje tylko obraz serwera centralnego
- `docker-compose-full.yml` - Pełny stack OtterWatch (db + mqtt + server + ai)
- `.env.example` - Przykładowa konfiguracja zmiennych środowiskowych

## Przygotowanie

### 1. Ustawienie zmiennych środowiskowych

```bash
# Skopiuj przykładowy plik .env
cp .env.example .env

# Edytuj wartości (szczególnie API keys i hasła!)
nano .env
```

### 2. Wybór narzędzia do budowania

Skrypty obsługują zarówno Docker jak i Podman:

```bash
export BUILD_TOOL=docker  # lub 'podman'
```

## Budowanie obrazów

### Opcja 1: Zbuduj wszystkie obrazy naraz

```bash
./build-all-images.sh
```

Domyślnie używa:
- Registry: `localhost:5000`
- Version: `latest`
- Tool: `docker`

### Opcja 2: Zbuduj indywidualne obrazy

```bash
# Tylko MQTT broker
./build-mqtt.sh

# Tylko serwer centralny
./build-server.sh
```

### Opcja 3: Budowanie z parametrami

```bash
# Własny registry i wersja
REGISTRY=myregistry.com VERSION=0.3.6 ./build-all-images.sh

# Używając Podman zamiast Docker
BUILD_TOOL=podman ./build-all-images.sh

# Kombinacja
BUILD_TOOL=podman REGISTRY=docker.io/myuser VERSION=v1.0.0 ./build-mqtt.sh
```

## Uruchamianie stacku

### Pełny stack z Docker Compose

```bash
cd /home/ximot/RustroverProjects/otterwatch/scripts

# Uruchom wszystko (db + mqtt + server)
docker-compose -f docker-compose-full.yml up -d

# Sprawdź status
docker-compose -f docker-compose-full.yml ps

# Zobacz logi
docker-compose -f docker-compose-full.yml logs -f

# Zatrzymaj
docker-compose -f docker-compose-full.yml down
```

### Używając Podman Compose

```bash
podman-compose -f docker-compose-full.yml up -d
```

## Zbudowane obrazy

Po pomyślnym buildzie otrzymasz:

```
localhost:5000/otterwatch-mqtt:latest
localhost:5000/otterwatch-mqtt:0.1.0

localhost:5000/otterwatch-server:latest  
localhost:5000/otterwatch-server:0.3.6
```

## Push do registry

```bash
# Login do registry (jeśli wymagane)
docker login localhost:5000

# Push konkretnego obrazu
docker push localhost:5000/otterwatch-mqtt:latest
docker push localhost:5000/otterwatch-server:latest

# Lub wszystkie naraz
docker push localhost:5000/otterwatch-mqtt:latest
docker push localhost:5000/otterwatch-mqtt:0.1.0
docker push localhost:5000/otterwatch-server:latest
docker push localhost:5000/otterwatch-server:0.3.6
```

## Porty i endpointy

Po uruchomieniu stacku:

| Serwis | Port | Endpoint | Opis |
|--------|------|----------|------|
| PostgreSQL | 5432 | `postgres://otterwatch:otterwatch@localhost:5432/otterwatch` | Baza danych |
| MQTT Broker | 1883 | `tcp://localhost:1883` | MQTT protocol |
| MQTT API | 8085 | `http://localhost:8085` | Management API |
| MQTT Metrics | 9090 | `http://localhost:9090/metrics` | Prometheus |
| Server API | 8080 | `http://localhost:8080` | REST API + Dashboard |

## Testowanie

### Sprawdź health wszystkich serwisów

```bash
# PostgreSQL
pg_isready -h localhost -U otterwatch

# MQTT API
curl http://localhost:8085/health

# Server
curl http://localhost:8080/health
```

### Test MQTT połączenia

```bash
# Zainstaluj mosquitto-clients
apt-get install mosquitto-clients

# Subscribe do topicu
mosquitto_sub -h localhost -p 1883 -t 'otterwatch/#' -u '' -P 'your-api-key'

# Publish test message
mosquitto_pub -h localhost -p 1883 -t 'otterwatch/test' -m 'hello' -u '' -P 'your-api-key'
```

## Troubleshooting

### Problem: Błąd "connection refused"

Sprawdź czy kontenery są uruchomione:
```bash
docker-compose -f docker-compose-full.yml ps
```

### Problem: MQTT authentication failed

Sprawdź czy API key się zgadza w:
- `.env` (zmienna `API_KEYS`)
- `.env` (zmienna `MQTT_API_KEYS`)
- Konfiguracji agenta (`settings.toml`)

### Problem: Server nie może połączyć się z bazą

Poczekaj aż PostgreSQL przejdzie healthcheck:
```bash
docker-compose -f docker-compose-full.yml logs db
```

### Podejrzyj logi konkretnego serwisu

```bash
docker-compose -f docker-compose-full.yml logs -f mqtt
docker-compose -f docker-compose-full.yml logs -f server
docker-compose -f docker-compose-full.yml logs -f db
```

## Czyszczenie

### Zatrzymaj i usuń kontenery (dane pozostają)

```bash
docker-compose -f docker-compose-full.yml down
```

### Usuń także volume'y (OSTRZEŻENIE: usuwa dane!)

```bash
docker-compose -f docker-compose-full.yml down -v
```

### Usuń obrazy

```bash
docker rmi localhost:5000/otterwatch-mqtt:latest
docker rmi localhost:5000/otterwatch-server:latest
```

## Produkcja

Dla środowiska produkcyjnego:

1. **Zmień hasła i API keys** w `.env`
2. **Użyj własnego registry**: `REGISTRY=myregistry.com`
3. **Ustaw konkretną wersję**: `VERSION=0.3.6`
4. **Skonfiguruj SSL/TLS** dla MQTT i HTTPS
5. **Ustaw proper `PUBLIC_URL`** dla serwera
6. **Zwiększ retencję danych**: `metrics_retention_days`
7. **Skonfiguruj backupy** PostgreSQL volume'u

## Zmienne środowiskowe

Pełna lista w `.env.example`:

```bash
# Registry i wersje
REGISTRY=localhost:5000
VERSION=latest
BUILD_TOOL=docker

# PostgreSQL
POSTGRES_USER=otterwatch
POSTGRES_PASSWORD=otterwatch
POSTGRES_DB=otterwatch
DB_PORT=5432

# MQTT
MQTT_API_KEYS=["your-api-key"]
MQTT_PORT=1883
MQTT_API_PORT=8085

# Server  
API_KEYS=your-api-key
SERVER_PORT=8080
PUBLIC_URL=http://localhost:8080

# Logi
MQTT_LOG_LEVEL=info
SERVER_LOG_LEVEL=info
```

## Wsparcie

- Issues: GitHub repository
- Docs: `CLAUDE.md` w głównym folderze projektu
