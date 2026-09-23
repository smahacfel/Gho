# M6: wersjonowane pomiary Sybil

`MaterializedFeatureSet.sybil_resistance` zachowuje historyczne skalary. `fee_topology_diversity_index` nadal oznacza K/N, a `demand_elasticity_score` DES V1. Nie są aliasami nowych definicji.
Nowy FTDI znajduje się w `fee_topology_diversity_v2` (`definition: gini_simpson`); DES w `demand_elasticity_v2` (`next_buy_slot_tau_b`, sloty, raw Pump post-trade reserves). DBIA i SFD mają rekordy `dbia_evidence_v1` oraz `sfd_evidence_v1`.
Każdy rekord przenosi wynik, liczność populacji, liczność użytej próby i własne powody degradacji. Jakość wynika z tych danych; samo `Some(value)` nie autoryzuje porównania. Brak jednej metryki nie wyłącza niezależnych sygnałów.
`measurement_cutoff_received_ms` identyfikuje cutoff nowej materializacji. Wcześniej wydane obiekty pozostają niezmienne, lecz nowy odczyt może mieć nowy cutoff i wynik.

## Porównania
Grupa `GatekeeperV2Config.sybil_thresholds_v2` jest domyślnie pusta. Stare progi nie są automatycznie przenoszone. Brak lub niedopasowanie progu daje jawny reason porównania, nie zerowy pomiar, premię ani karę.

| Opcja | Zakres | Odbiorca |
|---|---|---|
| `ftdi_gini_simpson_min` | [0,1] | soft signal FTDI |
| `des_next_buy_slot_tau_b_min` | [-1,1] | soft signal DES |
| `prosperity_branch3_ftdi_gini_simpson_min` | [0,1] | Prosperity B3 |
| `prosperity_overlay_ftdi_gini_simpson_min` | [0,1] | overlay Prosperity |

APS ma osobne `aps.ftdi_gini_simpson_v2_min`. Nie dziedziczy historycznego 0.0909. Niepełny CPV lub brak wymaganego porównania pozostawia shadow result jako `None`.
Wartości testowe nie są kalibracją ani zalecanymi ustawieniami produkcyjnymi. M6 nie zmienia plików konfiguracji operatora ani wzorów scoringów.

## Zapis i odczyt
Pełne evidence i projekcja FTDI mają osobny `gini_simpson_v2`; walidatory K/N pozostają nienaruszone. Compact Wire V1 zachowuje tuple 7-elementowy FTDI, a Wire V2 dodaje ósmy element z nowym rekordem. Stare golden hashe V1 pozostają testowane; zmiana wersji bez zmiany kształtu jest odrzucana.
Log decyzji dodaje `sybil_measurements_v2` z gotowymi pomiarami, cutoff, progami i powodami porównań. Hash nowego snapshotu obejmuje wersjonowane evidence i jakość; historyczne rekordy bez tych pól zachowują dotychczasowe znaczenie.

## Ograniczony replay
`cargo run --offline --locked -p ghost-launcher --example m6_bounded_replay -- <raw-v2-directory> <new-output-directory>` czyta wyłącznie pierwszy segment związany z receipt, maksymalnie 2048 rekordów/64 MiB. Sprawdza hashe przed i po odczycie, korzysta z istniejącego normalizatora/parsera/producentów i zapisuje ograniczenia oraz powody braków. Nie łączy się z siecią, nie korzysta z wyników transakcji inwestycyjnych, nie wykonuje zleceń i nie używa FSC jako bramki.

## Odbiorca diagnostyczny selectora
Historyczne zakresy normalizacji i wzór sumowania selectora nie zostały zmienione. W nowoczesnym logu FTDI/DES nie są podstawiane pod historyczne cechy selectora; jego `metric_comparison_disabled_reasons` ujawnia niezgodność definicji. DBIA/SFD/CPV wymagają własnego kompletnego evidence i zgodności z płaskim polem. Pozostałe cechy pozostają dostępne. Wagi i progi zbiorczego scoringu nie są kalibrowane w M6.
Próg FTDI overlay nieobecny w nowej konfiguracji wyłącza tylko to porównanie (`None`), nie pozostałe bramki Prosperity. Próg skonfigurowany przy niepełnych danych nadal wymaga dostępnego pomiaru; nie jest pomijany.
