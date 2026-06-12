---
status: partial
phase: 73-smart-alarm-wake-window-engine
source:
  - .planning/phases/73-smart-alarm-wake-window-engine/73-01-SUMMARY.md
  - .planning/phases/73-smart-alarm-wake-window-engine/73-02-SUMMARY.md
started: 2026-06-12T18:45:00Z
updated: 2026-06-12T22:42:00Z
---

## Current Test

[testing complete]

## Tests

### 1. Wake Alarm section visible in Sleep Coach
expected: Abre o tab Sleep Coach. No final do ecrã (após fazer scroll) deve aparecer um card com título "ALARME DE DESPERTAR".
result: pass
notes: Secção "ALARME DE DESPERTAR" visível em Sleep Coach. DatePicker (7:00 AM) e botão "Armar Alarme" presentes. Verificado via simulador.

### 2. Disconnected state UI
expected: Com o WHOOP desligado/não conectado: o DatePicker aparece com opacidade reduzida (dimmed), o botão "Armar Alarme" está disabled (não responde a taps), e aparece a mensagem "Conecta o WHOOP para usar o alarme".
result: pass
notes: UI hierarchy confirma enabled=false no DatePicker (help="Conecta o WHOOP para ativar") e no botão. Mensagem "Conecta o WHOOP para usar o alarme" visível. Tap no botão não produziu efeito.

### 3. Arm alarm (com WHOOP conectado)
expected: Com o WHOOP conectado, seleciona uma hora no DatePicker e toca em "Armar Alarme". O botão muda para vermelho com texto "Cancelar Alarme" e o DatePicker fica desativado (não editável).
result: blocked
blocked_by: physical-device
reason: "Requer WHOOP conectado via BLE — não testável no simulador iOS"

### 4. Cancel alarm
expected: Com o alarme armado (botão mostra "Cancelar Alarme"), toca no botão. O botão volta ao indigo com texto "Armar Alarme" e o DatePicker fica novamente editável.
result: blocked
blocked_by: physical-device
reason: "Requer WHOOP conectado via BLE — depende do teste 3"

### 5. Disconnect clears armed state
expected: Com o alarme armado, desliga o WHOOP (ou vai às Definições de Bluetooth e desconecta). A UI deve resetar: botão volta a "Armar Alarme" em indigo, sem necessidade de qualquer interação manual.
result: blocked
blocked_by: physical-device
reason: "Requer WHOOP conectado via BLE — depende do teste 3"

## Summary

total: 5
passed: 2
issues: 0
pending: 0
skipped: 0
blocked: 3

## Gaps

[none]
