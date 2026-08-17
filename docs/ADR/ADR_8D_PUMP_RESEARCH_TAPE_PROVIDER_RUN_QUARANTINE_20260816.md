# ADR-8D: Pump Research Evidence Tape V1.1 — kwarantanna historycznych provider-backed raw runów

**Data:** 2026-08-16

**Status:** FORENSIC PRESERVATION / QUALIFICATION NO-GO / NO RETROACTIVE PROMOTION

**Task:** `PUMP_RESEARCH_TAPE_PROVIDER_RUN_QUARANTINE`

## D0. Cel

Ten ADR zapisuje faktyczny stan wykonanych artefaktów PR-A bez zmiany ich
bytes. Nie jest receipt qualification, nie zastępuje independent source audit
i nie autoryzuje strategii.

## D1. Zachowane runy

### `pump-research-1786810400363-3428808`

W katalogu `raw/` istnieją tylko start manifest, run-local provenance binding
i `segment_00000.bin.partial`. Nie istnieje completion receipt.

Klasyfikacja:

```text
INCOMPLETE / interrupted capture / not certifiable
```

`.partial` pozostaje dowodem przerwanego lifecycle. Nie wolno go usuwać,
traktować jak opublikowany segment ani materializować jako source qualification.

### `pump-research-1786810567606-3429034`

`run_completion_receipt.json` zapisuje następujące, lokalnie sprawdzalne
fakty lifecycle:

```text
status / clean shutdown / source stream established  = Complete / true / true
received / admitted / persisted source records       = 1,383,849 / 1,383,849 / 1,383,849
dropped source updates / local gaps                  = 0 / 0
persisted ingress gap episodes / missing events      = 0 / 0
closed segments                                      = 20
source workers clean / lifecycle error / failure     = true / null / null
```

Historyczny binding wskazuje:

```text
release binary SHA-256       = 79b7caaf7f29529e420834f8cccd7a764674cadf7e907ec6e28c33b651591f7f
preflight receipt SHA-256    = b7f685ab4f9a6c28f0cbbd33cccac14dbdc4e6a6905a3ad7ba68af063b89574f
external config SHA-256      = 51dd7ac1505e822d517b5c8d2483766af9209077f6f42e1ac56ee75a90213997
artifact provenance SHA-256  = aa02ee8e51564a3268d2db409a6bd5e3d06ec29201832826eb69c3a621093a8d
```

Te digests identyfikują historyczny receipt; nie dowodzą obecnej hermetycznej
build/auth policy.

Klasyfikacja:

```text
CaptureLifecycleComplete
+ LocalAccountingComplete
+ ProvenancePreflightVulnerable
+ IndependentCompletenessUnproven
+ QualificationNoGo
```

Przyczyną `QualificationNoGo` są wykryte później luki: credential był widoczny
dla inherited fresh-build child environment, a PR-A no-auth ProgramData RPC
nie był jeszcze odseparowany od legacy auth fallbacku. Nie twierdzimy, że
credential rzeczywiście wyciekł albo że niejawny RPC header został wysłany;
oba fakty pozostają odpowiednio niewykazane/warunkowe. To wystarcza, by
odmówić promotion artefaktu.

## D2. Granice użycia

Runy mogą zostać zachowane do forensics i developmentu offline. Nie wolno na
nich wykonać promocji do Ready, independent qualification jako replacement
dowodu, `export-window`, eksperymentu strategii ani retroaktywnego podmieniać
preflight receipt. Nowy receipt nie zmienia provenance starych raw segmentów.

Przed replacement capture operator powinien lokalnie sprawdzić stare bundle
pod kątem bytes historycznego credentialu i rozważyć jego rotację. Wartość
credentialu nie może trafić do repo, ADR-a, logu ani rozmowy.

## D3. Niezmienność i następstwo

Nie zmieniono żadnego raw segmentu, footeru, manifestu ani existing exact
output. Następny materiał kwalifikacyjny musi pochodzić z nowego sealed
preflightu po build/auth isolation correction, krótkiego canary, inspection
i osobnego operator GO.
