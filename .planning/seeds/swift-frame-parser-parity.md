---
name: swift-frame-parser-parity
description: Cherry-pick WhoopFrameParser do PR #50 upstream — elimina round-trip FFI no hot path BLE, com testes de paridade Swift↔Rust obrigatórios antes de activar
metadata:
  type: project
  trigger_condition: Fases 68 (BLE Data Validator) e 69 (SQLite v20) fechadas
  planted_date: 2026-06-12
  source_pr: https://github.com/b-nnett/goose/pull/50
---

## Ideia

O PR #50 do upstream (naz3eh) introduz `WhoopFrameParser.swift` — substituto drop-in de `NotificationFrameParser` que elimina o round-trip hex→JSON→FFI→Rust→JSON por cada notificação BLE. O README admite lag "very considerable"; este é o fix de performance mais directo.

## Valor

- Afecta todos os dispositivos (WHOOP 4, 5, MG) — não é feature de nicho.
- Elimina C FFI do hot path de notificações em tempo real.
- A parte MG (comandos Labrador ECG 124/125/139, K16) não tem valor sem hardware MG — SKIP por agora.

## Por que não mergear agora

- A fase 68 (BLE Data Validator, commit 3ea93e8) injectou `GooseBLEDataValidator` em `GooseAppModel.swift:64` — exactamente onde o PR troca `NotificationFrameParser` por `WhoopFrameParser`. Colisão directa.
- Sem testes de paridade Swift↔Rust: a fórmula motion intensity está espalhada por dois sítios no Rust (`bridge.rs` + `protocol.rs`); alta probabilidade de divergência silenciosa.
- Offsets K10 hardcoded (85/285/485/688/888/1088) correctos agora mas frágeis a firmware updates.

## Plano quando activar

1. Cherry-pick só `WhoopFrameParser.swift` do branch `naz3eh:fix/backend`.
2. Escrever testes golden de paridade: mesmos fixtures pelo Rust bridge e pelo Swift parser, assert igualdade do `NotificationFrameCompactSummary`.
3. Integrar `WhoopFrameParser` *através* do `GooseBLEDataValidator` da fase 68, não substituindo o pipeline.
4. Ignorar a Parte B (MG support) — arquivar em seed separada se/quando houver hardware MG.

## Trigger

Activar quando fases 68 e 69 estiverem fechadas e verificadas.
