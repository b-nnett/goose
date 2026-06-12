# Requirements: Goose v10.0

**Defined:** 2026-06-12
**Core Value:** The user captures WHOOP data on iPhone and it is automatically persisted on their personal server — without depending on external infrastructure. Metrics align with WHOOP from the same raw data.

## v10.0 Requirements

### BLE5 — WHOOP 5.0 Protocol Parity

- [ ] **BLE5-01**: Utilizador com WHOOP 5.0 vê métricas em tempo real (R22 type 0x10 parsed; R17/R22 dual-stream dedup incluído na mesma fase)
- [ ] **BLE5-02**: Histórico por segundo WHOOP 5.0 importado sem duplicados (v18 decode + stale-clock dedup via sequence_id)
- [x] **BLE5-03**: Sync histórico BLE gerido por GooseBLEHistoricalManager dedicado (desacoplado de GooseBLEClient; proxy computed property preserva call sites)
- [x] **BLE5-04**: Frame BLE inválida rejeitada antes de chegar ao Rust/SQLite (GooseBLEDataValidator — invariantes estruturais apenas, sem packet-type whitelist)

### HAP — Haptics / Hardware WHOOP 5.0

- [x] **HAP-01**: App consegue vibrar a pulseira WHOOP 5.0 via BLE cmd 0x13 (buzz(loops:) primitive; pré-requisito para HAP-02/03/04 e FEAT-02)
- [ ] **HAP-02**: Utilizador consegue usar ecrã Breathe com feedback haptic paceado (AdvancedHaptic/HapticHeartbeat; requer HAP-01)
- [x] **HAP-03**: Utilizador consegue agendar alarme de vibração na pulseira a hora fixa (smart alarm — single-shot BLE write; requer HAP-01)
- [ ] **HAP-04**: Pulseira vibra no momento óptimo dentro de uma janela de despertar (wake-window engine; RE-gated — requer sessões BTSnoop + Ghidra de SetAlarmInfoCommandPacketRev4)

### FEAT — Features / Coach / Notificações

- [ ] **FEAT-01**: Coach tab mostra nudges VOW (Voice of WHOOP) contextuais calculados localmente via bridge (sem servidor)
- [ ] **FEAT-02**: Utilizador consegue aceder a Breathe UI, Interval Timer e Metric Explorer (NoopApp features; Breathe UI requer HAP-01)
- [x] **FEAT-03**: App envia notificação local após conclusão de ciclo de sono, detecção de workout e bateria WHOOP abaixo de 20% (usa getNotificationSettings — permissão já concedida em onboarding)

### DATA — Dados / Ecrãs

- [ ] **DATA-01**: App persiste diário de comportamentos (Y/N diários), log de treino com sport tag, dados Apple Health diários, e séries de métricas genéricas em SQLite (schema v20 — 4 tabelas com migration arm condicional)
- [ ] **DATA-02**: Ecrã de workout mostra strain acumulado em tempo real durante sessão activa (GooseStrainAccumulator Swift-side; publica via Task @MainActor)
- [x] **DATA-03**: Utilizador vê ecrã Stress/ANS com tiles ANS, dashboard Trends histórico e sheet de entrada manual de workout
- [x] **DATA-04**: Ecrã de HR carrega sem lag em sessões longas (HR sample decimation via stride/LTTB preservando extremos locais)

### ARCH — Arquitectura / Testabilidade

- [ ] **ARCH-01**: GooseBLEClient, GooseRustBridge e HealthDataStore têm protocolos Swift e mocks correspondentes no target de testes; parâmetros com default preservam call sites existentes

## Future Requirements

Deferred — not in v10.0 scope:

### Hardware Gate

- CAPSENSE-01: Cap sense GATT UUID + on-wrist detection (WHPWhoopStrapOnWrist parity) — hardware gate; UUID não identificado via Ghidra
- HAP-04 RE prerequisite: BTSnoop captura de `STRAP_DRIVEN_ALARM_EXECUTED` + Ghidra decompile de `SetAlarmInfoCommandPacketRev4` — necessário antes de planear HAP-04

### Out of Scope (v10.0)

- Android app features — architecture foundations only (v2.0 decision)
- Server-side data analysis or dashboards
- Background URLSession upload when app suspended
- Upload queue persisted across app restarts
- Advanced authentication (OAuth, 2FA)
- WHOOP 4.0-specific haptic commands (different cmd set)
- CoreHaptics / Taptic Engine integration (strap uses BLE cmd 0x13, not iOS haptic engine)
- Full NoopApp feature parity beyond Breathe, Interval Timer, Metric Explorer

## Traceability

| REQ-ID | Phase | Notes |
|--------|-------|-------|
| BLE5-01 | Phase 67 | R22 type 0x10 + R17/R22 dedup — pure Rust, protocol.rs |
| BLE5-02 | Phase 67 | v18 per-second decode + sequence_id dedup — pure Rust |
| BLE5-03 | Phase 68 | GooseBLEHistoricalManager decoupled from GooseBLEClient |
| BLE5-04 | Phase 68 | GooseBLEDataValidator Swift struct — structural invariants only |
| HAP-01 | Phase 70 | buzz(loops:) cmd 0x13 — GooseBLEClient+Haptics.swift |
| HAP-02 | Phase 70 | Breathe screen + paced haptic cues; depends on HAP-01 |
| HAP-03 | Phase 73 | Single-shot alarm UI; depends on HAP-01 |
| HAP-04 | Phase 73 | RE-gated wake-window engine; SetAlarmInfoCommandPacketRev4 required |
| FEAT-01 | Phase 71 | Coach VOW nudges — local bridge decision tree |
| FEAT-02 | Phase 71 | Breathe UI + Interval Timer + Metric Explorer; Breathe UI depends on HAP-01 |
| FEAT-03 | Phase 71 | iOS local notifications — NotificationScheduler actor |
| DATA-01 | Phase 69 | 4 SQLite tables — schema v20 migration in store.rs |
| DATA-02 | Phase 69 | GooseStrainAccumulator Swift actor — realtime strain during workout |
| DATA-03 | Phase 72 | Stress/ANS view + Trends dashboard + Manual Workout Entry sheet |
| DATA-04 | Phase 71 | HR decimation via stride/LTTB in HeartRateSeriesStore |
| ARCH-01 | Phase 72 | GooseBLEManaging + GooseRustBridging + HealthDataStoring protocols + mocks + 2 tests |
