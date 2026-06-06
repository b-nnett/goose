import SwiftUI
import WebKit

// MARK: - GeminiOAuthWebView

struct GeminiOAuthWebView: UIViewRepresentable {
  let clientId: String
  let codeVerifier: String
  let onCode: (String) -> Void

  private var authURL: URL {
    let codeChallenge = GeminiCoachProvider.codeChallenge(for: codeVerifier)
    var components = URLComponents(string: "https://accounts.google.com/o/oauth2/v2/auth")!
    components.queryItems = [
      URLQueryItem(name: "client_id", value: clientId),
      URLQueryItem(name: "redirect_uri", value: "gooseswift://oauth/gemini"),
      URLQueryItem(name: "response_type", value: "code"),
      URLQueryItem(name: "scope", value: "https://www.googleapis.com/auth/generative-language"),
      URLQueryItem(name: "code_challenge", value: codeChallenge),
      URLQueryItem(name: "code_challenge_method", value: "S256"),
      URLQueryItem(name: "access_type", value: "offline"),
    ]
    return components.url!
  }

  func makeCoordinator() -> Coordinator {
    Coordinator(onCode: onCode)
  }

  func makeUIView(context: Context) -> WKWebView {
    let webView = WKWebView()
    webView.navigationDelegate = context.coordinator
    return webView
  }

  func updateUIView(_ webView: WKWebView, context: Context) {
    guard webView.url == nil else { return }
    let request = URLRequest(url: authURL)
    webView.load(request)
  }

  // MARK: - Coordinator

  final class Coordinator: NSObject, WKNavigationDelegate {
    private let onCode: (String) -> Void

    init(onCode: @escaping (String) -> Void) {
      self.onCode = onCode
    }

    func webView(
      _ webView: WKWebView,
      decidePolicyFor navigationAction: WKNavigationAction,
      decisionHandler: @escaping (WKNavigationActionPolicy) -> Void
    ) {
      guard let url = navigationAction.request.url,
            url.scheme == "gooseswift" else {
        decisionHandler(.allow)
        return
      }

      decisionHandler(.cancel)

      // Extract authorization code from redirect URL
      guard let components = URLComponents(url: url, resolvingAgainstBaseURL: false),
            let codeItem = components.queryItems?.first(where: { $0.name == "code" }),
            let code = codeItem.value else {
        return
      }

      onCode(code)
    }
  }
}
