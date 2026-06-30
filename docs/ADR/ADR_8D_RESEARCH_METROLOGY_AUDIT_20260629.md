# ADR-8D: Research Metrology Audit 2026-06-29

## Status

IMPLEMENTED / OFFLINE_AUDIT / METROLOGY_PASS_WITH_WARNINGS

## Kontekst

ORG-A0, TSV2 A1/A2/A3, EIX, RTP-A0, RUG-MARKUP-A0 i RCE-A0 sa liniami
badawczymi opartymi o replay, lifecycle, CSV metryki i konfiguracje scope.
Przed dalsza interpretacja wynikow potrzebny jest P0 audit metrologiczny:
czy symulator, lifecycle join, metryki, horyzonty i konfiguracje nie robia z
wynikow artefaktu pomiarowego.

## Decyzja

Dodano offline-only audit:

- `scripts/research_metrology_audit.py`
- `reports/selector/research_metrology_audit_*.csv`
- `PLANS/AUDYT/RAPORT_RESEARCH_METROLOGY_AUDIT_20260629.md`

Final verdict: **METROLOGY_PASS_WITH_WARNINGS**

Runtime approval: **false**
Shadow close approval: **false**
Active close approval: **false**

## Konsekwencje

Poprzednie negatywne wyniki pozostaja wazne w audytowanych horyzontach i przy jawnych ograniczeniach pomiaru.

Nie wolno inferowac wnioskow dla 300000/500000 ms, jezeli coverage jest NOT_EVALUABLE.

Raw JSONL logs pozostaja lokalnym evidence i nie sa committowane.

## Guardrails

- no runtime change
- no BUY/REJECT change
- no Gatekeeper policy change
- no selector runtime change
- no TX/Jito/live path change
- no cleanup
- no raw JSONL commit
