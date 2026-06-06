import Foundation


// MARK: - Display-layer localisation for dynamic @Published status strings
//
// Raw @Published values (e.g. "disconnected", "idle", "poweredOn") are state-machine
// constants — they must remain unchanged in GooseBLEClient and GooseAppModel.
// These extension methods are the only translation boundary: call .localizedXxx only
// at view display sites (Text, LabeledContent value:, statusText). Never use them in
// guards, comparisons, or switch statements that drive control flow (Pitfall 2 / D-03).

extension String {

  // MARK: - BLE Connection State (GooseBLEClient.connectionState)
  // Raw values: "disconnected", "connecting", "connected", "discovering", "ready"

  var localizedConnectionState: String {
    switch self {
    case "disconnected": return String(localized: "Disconnected")
    case "connecting": return String(localized: "Connecting")
    case "connected": return String(localized: "Connected")
    case "discovering": return String(localized: "A descobrir...")
    case "ready": return String(localized: "Ligado")
    default: return self
    }
  }

  // MARK: - HR Monitor Connection State (GooseBLEClient.hrConnectionState)
  // Raw values: "disconnected", "connecting", "connected"

  var localizedHRConnectionState: String {
    switch self {
    case "disconnected": return String(localized: "Disconnected")
    case "connecting": return String(localized: "Connecting")
    case "connected": return String(localized: "Connected")
    default: return self
    }
  }

  // MARK: - Bluetooth State (GooseBLEClient.bluetoothState)
  // Raw values: "not requested", "powered on", "powered off", "unauthorized",
  //             "unsupported", "resetting", "unknown", "bluetooth unavailable"

  var localizedBluetoothState: String {
    switch self {
    case "powered on":   return String(localized: "Ativo")
    case "powered off":  return String(localized: "BLUETOOTH OFF")
    case "unauthorized": return String(localized: "NOT AUTHORISED")
    case "unsupported":  return String(localized: "Não suportado")
    case "resetting":    return String(localized: "A reiniciar...")
    case "not requested": return String(localized: "Não solicitado")
    case "unknown":      return String(localized: "Desconhecido")
    case "bluetooth unavailable": return String(localized: "NOT AUTHORISED")
    default: return self
    }
  }

  // MARK: - HR Bluetooth State (GooseBLEClient.hrBluetoothState)
  // Raw values match centralManagerDidUpdateState: "poweredOn", "poweredOff",
  //             "unauthorized", "unsupported", "resetting", "unknown"

  var localizedHRBluetoothState: String {
    switch self {
    case "poweredOn":    return String(localized: "Ativo")
    case "poweredOff":   return String(localized: "BLUETOOTH OFF")
    case "unauthorized": return String(localized: "NOT AUTHORISED")
    case "unsupported":  return String(localized: "Não suportado")
    case "resetting":    return String(localized: "A reiniciar...")
    case "unknown":      return String(localized: "Desconhecido")
    default: return self
    }
  }

  // MARK: - Reconnect State (GooseBLEClient.reconnectState)
  // Raw values include "idle", "already connected", "forgotten", "remembered rejected",
  //   "blocked", "waiting for bluetooth", "no remembered device", "retrieving remembered",
  //   "remembered was not WHOOP", "scanning for remembered WHOOP", "scanning for remembered",
  //   "scanning for WHOOP physiology", "failed after 10 attempts", "connecting",
  //   "reconnecting (attempt N/10)" (from ReconnectBackoff.statusString)

  var localizedReconnectState: String {
    switch self {
    case "idle":               return String(localized: "Inativo")
    case "already connected":  return String(localized: "Já ligado")
    case "connecting":         return String(localized: "A ligar...")
    case "failed after 10 attempts": return String(localized: "Falhou após 10 tentativas")
    case "blocked":            return String(localized: "Bloqueado")
    case "waiting for bluetooth": return String(localized: "À espera de Bluetooth")
    case "no remembered device": return String(localized: "Nenhum dispositivo guardado")
    case "retrieving remembered": return String(localized: "A recuperar dispositivo...")
    case "remembered was not WHOOP": return String(localized: "Dispositivo guardado não é WHOOP")
    case "scanning for remembered WHOOP": return String(localized: "A procurar WHOOP guardado...")
    case "scanning for remembered": return String(localized: "A procurar dispositivo guardado...")
    case "scanning for WHOOP physiology": return String(localized: "A procurar fisiologia WHOOP...")
    case "forgotten": return String(localized: "Esquecido")
    case "remembered rejected": return String(localized: "Dispositivo recusado")
    default:
      if self.hasPrefix("reconnecting") {
        return String(localized: "A tentar ligar...")
      }
      return self
    }
  }

  // MARK: - HR Reconnect State (GooseBLEClient.hrReconnectState)
  // Shares the same ReconnectBackoff pattern; simpler value set in practice.

  var localizedHRReconnectState: String {
    switch self {
    case "idle":              return String(localized: "Inativo")
    case "already connected": return String(localized: "Já ligado")
    case "failed after 10 attempts": return String(localized: "Falhou após 10 tentativas")
    default:
      if self.hasPrefix("reconnecting") {
        return String(localized: "A tentar ligar...")
      }
      return self
    }
  }

  // MARK: - Historical Sync Status (GooseBLEClient.historicalSyncStatus)
  // Raw values: "idle", "syncing", "waiting", "synced", "failed"

  var localizedHistoricalSyncStatus: String {
    switch self {
    case "idle":    return String(localized: "Inativo")
    case "syncing": return String(localized: "A sincronizar...")
    case "waiting": return String(localized: "À espera...")
    case "synced":  return String(localized: "Sincronizado")
    case "failed":  return String(localized: "Falhou")
    default: return self
    }
  }

  // MARK: - Strap Clock Status (GooseBLEClient.strapClockStatus)
  // Raw values: "Not read", "Clock command already in flight",
  //   "Clock command blocked by alarm command", "Syncing clock",
  //   "Clock out by ...; syncing", "Clock in sync", "Clock synced",
  //   dynamic strings from clock offset calculations

  var localizedStrapClockStatus: String {
    switch self {
    case "Not read":    return String(localized: "Não lido")
    case "Syncing clock": return String(localized: "A sincronizar relógio...")
    case "Clock in sync": return String(localized: "Relógio em sincronia")
    case "Clock synced":  return String(localized: "Relógio sincronizado")
    case "Clock command already in flight": return String(localized: "Comando de relógio em curso")
    case "Clock command blocked by alarm command": return String(localized: "Relógio bloqueado por alarme")
    default: return self
    }
  }

  // MARK: - Battery Power Status (GooseBLEClient.batteryPowerStatus)
  // Raw values: "Unknown", "Charging (inferred)", dynamic summary strings from BLE parsing

  var localizedBatteryPowerStatus: String {
    switch self {
    case "Unknown":            return String(localized: "Desconhecido")
    case "Charging (inferred)": return String(localized: "A carregar (inferido)")
    default: return self
    }
  }

  // MARK: - Capture Status (GooseAppModel.healthPacketCaptureStatus)
  // Highly dynamic — pass through with known prefix localisation.

  var localizedCaptureStatus: String {
    switch self {
    case "No health packet capture": return String(localized: "Sem captura de pacotes")
    case "No active health packet capture": return String(localized: "Sem captura activa")
    default: return self
    }
  }

  // MARK: - Capture Target Summary (GooseAppModel.healthPacketCaptureTargetSummary)
  // Highly dynamic — passthrough with known base value.

  var localizedCaptureTargetSummary: String {
    switch self {
    case "No health packet capture": return String(localized: "Sem captura de pacotes")
    default: return self
    }
  }

  // MARK: - Overnight Guard Status (GooseAppModel.overnightGuardStatus)
  // Raw values include "Not started", "Already recording overnight guard",
  //   "Connect WHOOP first...", "Recording overnight guard", "Start failed: ...",
  //   "Start overnight guard before final sync", dynamic lifecycle strings.

  var localizedOvernightGuardStatus: String {
    switch self {
    case "Not started": return String(localized: "Não iniciado")
    case "Recording overnight guard": return String(localized: "A gravar guarda nocturna")
    case "Already recording overnight guard": return String(localized: "Já a gravar guarda nocturna")
    case "Start overnight guard before final sync": return String(localized: "Inicie a guarda nocturna antes da sincronização final")
    default: return self
    }
  }

  // MARK: - Activity Detection Status (GooseAppModel.activityDetectionStatus)
  // Raw values include "Watching for movement packets", "Movement detected; priming GPS",
  //   "Candidate <title> recording", "Candidate <title> stored", dynamic pipeline strings.

  var localizedActivityDetectionStatus: String {
    switch self {
    case "Watching for movement packets": return String(localized: "À escuta de pacotes de movimento")
    default: return self
    }
  }

  // MARK: - Packet Import Status (GooseAppModel.packetImportStatus)
  // Raw values include "No packet import", "Packet import failed", dynamic status strings.

  var localizedPacketImportStatus: String {
    switch self {
    case "No packet import":     return String(localized: "Sem importação de pacotes")
    case "Packet import failed": return String(localized: "Importação de pacotes falhou")
    default: return self
    }
  }
}
