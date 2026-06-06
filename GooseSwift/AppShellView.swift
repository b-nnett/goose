import SwiftUI

struct AppShellView: View {
  @EnvironmentObject private var model: GooseAppModel
  @EnvironmentObject private var router: AppRouter
  @StateObject private var healthStore = HealthDataStore()
  @State private var homeHealthPath: [HealthRoute] = []
  @State private var homeSelectedDate = Date()

  var body: some View {
    TabView(selection: tabSelection) {
      ForEach(GooseAppTab.allCases) { tab in
        tabNavigationStack(for: tab)
        .tabItem {
          Label(tab.title, systemImage: tab.systemImage)
        }
        .tag(tab)
      }
    }
    // Weak capture and onDisappear clear avoid a lifetime inversion: model
    // (lives the whole app) would otherwise pin the view-owned HealthDataStore.
    // Fire site (GooseAppModel.handleHistoricalSyncProgress) hops to @MainActor,
    // which is where runPacketInputs() on the @MainActor HealthDataStore expects
    // to be called.
    .onAppear {
      model.onHistoricalSyncCompleted = { [weak healthStore] in
        healthStore?.runPacketInputs()
      }
    }
    .onDisappear {
      model.onHistoricalSyncCompleted = nil
    }
  }

  private var tabSelection: Binding<GooseAppTab> {
    Binding {
      router.selectedTab
    } set: { newTab in
      if newTab == router.selectedTab {
        router.reselect(newTab)
        return
      }
      router.selectedTab = newTab
      model.recordUIAction("tab.selected", detail: newTab.title)
    }
  }

  @ViewBuilder
  private func tabNavigationStack(for tab: GooseAppTab) -> some View {
    if tab == .home {
      NavigationStack(path: $homeHealthPath) {
        tabContent(for: tab)
          .navigationDestination(for: HealthRoute.self) { route in
            HealthRouteDestinationView(route: route, store: healthStore, selectedDate: $homeSelectedDate)
          }
      }
    } else if tab == .health {
      NavigationStack(path: $router.healthPath) {
        tabContent(for: tab)
      }
    } else if tab == .more {
      NavigationStack(path: $router.morePath) {
        tabContent(for: tab)
      }
    } else {
      NavigationStack {
        tabContent(for: tab)
      }
    }
  }

  @ViewBuilder
  private func tabContent(for tab: GooseAppTab) -> some View {
    switch tab {
    case .home:
      HomeDashboardView(
        healthStore: healthStore,
        selectedDate: $homeSelectedDate,
        openHealthRoute: openHomeHealthRoute
      )
    case .health:
      HealthView(store: healthStore)
    case .coach:
      CoachView(healthStore: healthStore)
    case .more:
      MoreView(healthStore: healthStore)
    }
  }

  private func openHomeHealthRoute(_ route: HealthRoute) {
    homeHealthPath = [route]
  }
}

enum GooseAppTab: String, CaseIterable, Identifiable {
  case home
  case health
  case coach
  case more

  var id: String { rawValue }

  var title: String {
    switch self {
    case .home: "Home"
    case .health: "Health"
    case .coach: "Coach"
    case .more: "More"
    }
  }

  var systemImage: String {
    switch self {
    case .home: "house"
    case .health: "heart.text.square"
    case .coach: "sparkles"
    case .more: "ellipsis.circle"
    }
  }

}
