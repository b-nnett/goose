---
name: healthkit-bevel-sync
description: Escrever métricas computadas pelo Goose para Apple Health (HealthKit) — HRV SDNN, RHR, sleep stages, temperatura, workouts — para compatibilidade com Bevel e apps similares
metadata:
  type: project
  trigger_condition: Fases de métricas e sleep scoring estáveis; SQLite migration (fase 69) fechada
  planted_date: 2026-06-12
  source_issue: https://github.com/tigercraft4/goose/issues/109
---

## Contexto

Issue #109 (PascalTrm) pediu sync para "Apple Home" — clarificado nos comentários como **Apple Health (HealthKit)**. O reporter não tem subscrição WHOOP, pelo que a app oficial não sincroniza nada para HealthKit. Goose seria o único caminho para alimentar apps como Bevel, Athlytic, Training Today.

## Valor

- Utilizadores sem subscrição WHOOP ficam sem dados em qualquer app de saúde de terceiros.
- Bevel, Athlytic e similares lêem exclusivamente de HealthKit — sem este bridge, os dados ficam isolados no SQLite do Goose.
- O Goose já tem entitlement `com.apple.developer.healthkit`, workout write, e sleep import. A infra existe; falta o write de métricas computadas.

## Dados que Bevel precisa (da resposta do reporter)

| Métrica | Estado no Goose | Notas |
|---|---|---|
| HRV (SDNN) | Goose calcula SDNN | **Atenção:** guarda activa `healthkit_rmssd_must_not_be_written_as_sdnn` — SDNN pode ser escrito mas não confundir com RMSSD |
| Resting Heart Rate | Lido do WHOOP | Disponível |
| Sleep stages / duration | Sleep tracking presente | Disponível |
| Temperatura corporal | Suporte K18 | Disponível se capturado |
| HR contínuo + Workouts | Activity detection + workout write | Parcialmente implementado |
| Respiratory Rate, SpO2, Steps, VO2Max, Weight | Fora do scope actual | Não implementar nesta fase |

## Pontos de atenção

- SDNN ≠ RMSSD — o codebase já tem guarda para isto; respeitar ao implementar writes.
- Bevel usa SDNN como métrica base de HRV; escrever RMSSD como SDNN seria incorrecto e poderia corromper a baseline do utilizador.
- Scope mínimo viável: HRV (SDNN) + RHR + sleep. O resto pode vir em fases posteriores.
- A issue usa o template errado (enhancement em vez de feature request) — se avançar, pedir ao reporter para reabrir com Feature Request template.

## Trigger

Activar quando as métricas de HRV/sleep/recovery estiverem estáveis e a migração SQLite (fase 69) fechada. Avaliar se há interesse da comunidade (upvotes/comentários na issue #109).
