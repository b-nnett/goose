"""
analysis — insight modules for WHOOP biometric data.

Submodules (added incrementally across milestone tasks):
  units   — real-unit conversions (SpO2, skin temp, resp rate)  [Task 1.2]
  hrv     — HRV metrics (RMSSD, SDNN) from RR intervals          [Task 2.1]
  sleep   — sleep/wake detection + staging + daily summary       [Task 2.2]
  recovery — resting HR during sleep + HRV-driven recovery score [Task 2.3]
  strain  — WHOOP 0–21 cardiovascular strain (Edwards TRIMP/HRR)  [Task 2.3]
  activity — per-record gravity L2-delta motion intensity         [Task 2.3]
  exercise — retroactive workout detection (elevated HR + motion)  [Task 2.4]
"""
