import Foundation

final class GooseMessageStore: ObservableObject {
  @Published private(set) var messages: [GooseMessage] = []

  private let maximumMessages: Int
  private let flushInterval: TimeInterval
  private var pendingMessages: [GooseMessage] = []
  private var flushWorkItem: DispatchWorkItem?

  init(maximumMessages: Int, flushInterval: TimeInterval) {
    self.maximumMessages = maximumMessages
    self.flushInterval = flushInterval
  }

  func enqueue(_ message: GooseMessage) {
    guard Thread.isMainThread else {
      DispatchQueue.main.async { [weak self] in
        self?.enqueue(message)
      }
      return
    }

    pendingMessages.append(message)
    guard flushWorkItem == nil else {
      return
    }

    let workItem = DispatchWorkItem { [weak self] in
      self?.flush()
    }
    flushWorkItem = workItem
    DispatchQueue.main.asyncAfter(deadline: .now() + flushInterval, execute: workItem)
  }

  func flush() {
    guard Thread.isMainThread else {
      DispatchQueue.main.async { [weak self] in
        self?.flush()
      }
      return
    }

    flushWorkItem?.cancel()
    flushWorkItem = nil
    guard !pendingMessages.isEmpty else {
      return
    }

    // Concatenation is O(n+k); insert-at-0 with shift is also O(n) but forces a copy of the
    // entire backing store. Using + avoids mutating the existing buffer in-place.
    let merged = pendingMessages.reversed() + messages
    pendingMessages.removeAll(keepingCapacity: true)
    messages = merged.count > maximumMessages ? Array(merged.prefix(maximumMessages)) : Array(merged)
  }
}
