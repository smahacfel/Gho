# ADR-0098: Naprawa warunku `is_grpc_mode` w watchodgu — zombie guard nieaktywny przy `source_mode = "grpc"`

**Date:** 2026-04-14  
**Status:** Accepted  
**Author:** Ghost Father

---

## Context

Bot (ghost-launcher) przestawał efektywnie pracować po 20 minutach do kilku godzin. Z logów wynikało, że:

- `grpc_state=CONNECTED reconnects=173` — 173 reconnectów, ale nadal brak danych
- `age_grpc=6080700ms` — brak wiadomości gRPC od ~101 minut
- `age_ipc ≈ age_bus ≈ age_gk ≈ age_events` — wszystkie kanały ciche jednocześnie
- Logi watchdoga w pętli INFO co 60 sekund — **brak jakichkolwiek ERROR/FATAL**
- Proces **kontynuował działanie** bez exitowania mimo pełnej zombie state

Wewnętrzny transport watchdog (`SILENT_STALL_SECS = 2s`) wykrywał stalle i wykonywał reconnecty (łącznie 173), ale każda nowa sesja gRPC natychmiast wchodziła ponownie w stan ciszy.

Zewnętrzny watchdog (`watchdog.rs`) posiadał poprawną logikę zombie guard (`GRPC_ZOMBIE_EXIT_MS = 10 min`) ale **nigdy jej nie uruchamiał**.

---

## Root Cause — Diagnoza

W `ghost-launcher/src/main.rs` flaga `is_grpc_mode` dla watchdoga była obliczana jako:

```rust
let is_grpc_mode = config
    .seer
    .source_mode
    .as_ref()
    .map(|m| m.to_lowercase() == "geyser_grpc")  // ← wymagało DOKŁADNIE "geyser_grpc"
    .unwrap_or(false);
```

Config zarówno `config.toml` jak i `configs/rollout/paper-burnin.toml` zawierał:

```toml
source_mode = "grpc"
```

Porównanie `"grpc" == "geyser_grpc"` = **FALSE** → `is_grpc_mode = false`.

Skutek:
1. Cała gałąź detekcji zombie i stall gRPC w watchdogu była **całkowicie wyłączona**
2. `grpc_is_fresh` przy zombie state (age_grpc >> 30s) = `false` → pipeline stall detection też nieaktywna
3. Bot trwał w zombie state **w nieskończoność** bez żadnej reakcji watchdoga
4. Zombie guard (`GRPC_ZOMBIE_EXIT_MS`) istniał w kodzie ale nigdy nie mógł zadziałać

Rozbieżność była łatwa do przeoczenia, ponieważ `config.rs` (linie 704, 858) prawidłowo obsługuje aliasy `"grpc" || "geyser_grpc" || "g"` do walidacji i sprawdzania profilu, ale `main.rs` miał niezależną, niepełną kopię tego warunku.

---

## Decision

Linia `main.rs:2143` została poprawiona przez wyrównanie warunku do kanonicznych aliasów z `config.rs`:

```rust
// Przed
.map(|m| m.to_lowercase() == "geyser_grpc")

// Po
.map(|m| {
    let m = m.to_lowercase();
    m == "geyser_grpc" || m == "grpc" || m == "g"
})
```

Dodano też komentarz dokumentujący konieczność synchronizacji z `config.rs`.

---

## Architectural Impact

- **Watchdog zombie guard** (`GRPC_ZOMBIE_EXIT_MS = 10 min`) jest teraz aktywny dla `source_mode = "grpc"` — bot exituje z kodem 2 po 10 minutach zombifikacji
- **Startup guard** (GRPC subscribe timeout) działał na tej samej fladze — teraz też aktywowany
- **Pipeline stall detection** (exits 3/4) — pośrednio aktywowana przez ożywienie `grpc_is_fresh`
- Brak zmian w kontraktach, SSOT, konfiguracjach, logice gatekeeper ani żadnej innej warstwie systemu

---

## Risk Assessment

**Poziom ryzyka: NISKI** (fix jeden warunek, brak zmian logiki biznesowej)

Jedyna zmiana behawioralna: Bot teraz **exituje** (kod 2) gdy gRPC jest zombie przez 10 minut, zamiast trwać w nieskończoność. Pożądany i bezpieczny efekt.

Potencjalny regres: żaden — poprzedni stan był gorszy (bot nieaktywny bez autorecovery).

---

## Consequences

**Przed naprawą:**
- Bot wchodzi w zombie state → trwa w nieskończoność → operator musi ręcznie restartować
- Wszystkie mechanizmy watchdoga wyłączone mimo obecności w kodzie

**Po naprawie:**
- Bot wchodzi w zombie state → zombie guard fired po ~10 min → process::exit(2)
- Operator/supervisor może zautomatyzować restart (np. tmux respawn, systemd restart=always)

**Otwarta kwestia (poza scope'em tego fixa):** Brak automatycznego restartu po exit(2). Po wyjściu procesu z `cargo run` w tmux, bot pozostaje martwy do ręcznego restartu. Rozwiązanie: dodać pętlę restart w skrypcie uruchomieniowym lub skonfigurować systemd.

---

## Alternatives Considered

1. **Zmiana `source_mode` w configach** na `"geyser_grpc"` — odrzucone, bo "grpc" jest wartością kanoniczną udokumentowaną w config.rs i stosowaną we wszystkich testach
2. **Centralizacja warunku do jednej funkcji w config.rs** — sensowny refaktor, ale poza scope'em krytycznej naprawy; nie modyfikuje API config.rs
3. **Zwiększenie tolerancji reconnect / dłuższy circuit breaker** — nie naprawia root cause

---

## Validation Steps

1. ✅ Kompilacja `cargo build --release -p ghost-launcher` — sukces, brak błędów (Apr 14 20:18)
2. Przy kolejnym uruchomieniu: w logach powinien pojawić się `grpc_mode=true` w linii startowej watchdoga
3. Na zombie state: po max ~11 minutach powinien pojawić się `WATCHDOG FATAL: gRPC zombie` + exit code 2
4. `RUST_LOG=debug` może ujawnić dodatkowe diagnostyki transport

---

## Related

- `docs/ADR/` — wszystkie poprzednie ADR
- `ghost-launcher/src/components/watchdog.rs` — logika zombie guard  
- `ghost-launcher/src/config.rs:704,858` — kanoniczne aliasy `is_grpc`
- `off-chain/components/seer/src/grpc_connection.rs` — transport watchdog, circuit breaker
