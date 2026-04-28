Tak. W tej chwili problem jest taki, że naprawiliśmy kilka bottlenecków, ale **nie domknęliśmy miejsca, w którym naprawdę uciekają tx względem chain truth**.

Najkrócej:
- `99%` dotyczyło **parsera na tym, co już weszło do `seer`**
- a nie tego, czy `seer` zobaczył **prawie wszystkie realne tx z chaina**
- więc z punktu widzenia twojego celu to była **zła metryka sukcesu**

Gdzie dalej realnie mogą ginąć tx:
- **przed parserem**: filtr/subskrypcja w [grpc_connection.rs](/root/Ghost/off-chain/components/seer/src/grpc_connection.rs)
- **na mapowaniu i gatingu**: pending mapping / forward decision w [lib.rs](/root/Ghost/off-chain/components/seer/src/lib.rs)
- **na runtime filtrowaniu**: aktywność puli, okno, dalsze odrzucenia downstream
- **na truth side**: nadal nie mamy runtime’owego, twardego porównania `chain signatures` vs `seer signatures`

Czyli prawda jest taka:
**dalej nie wiemy dokładnie, który etap zjada brakujące 50%, bo nie zrobiliśmy signature-by-signature accounting przeciw chain truth.**  
Bez tego kalibracja progów faktycznie stoi na gównie.

Mój błąd był prosty:
- potraktowałem `parser_cov` jako proxy całego coverage
- a to nie odpowiadało na właściwe pytanie

Żeby wyjść z tej dupy, trzeba teraz zrobić tylko to, bez kolejnych półśrodków:
1. Dodać **runtime chain-truth audit** per pool/window.
2. Logować trzy zbiory sygnatur:
   - `chain_truth`
   - `seer_rx`
   - `seer_emitted`
3. Dla każdej brakującej sygnatury przypisać **konkretny reason code**:
   - `not_received`
   - `filtered_before_parse`
   - `parse_miss`
   - `mapping_missing`
   - `not_forwarded`
   - `runtime_filtered`
4. Dopiero na tym robić następny patch.

Bez tego dalej będziemy łatać w ciemno.

Jeśli chcesz, w następnym kroku robię już wyłącznie:
- **prawdziwy end-to-end coverage tracer**
- z porównaniem po `signature`
- i z rozbiciem, **na którym etapie ginie każdy tx**.