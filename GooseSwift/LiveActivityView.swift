import CoreLocation
import MapKit
import SwiftUI
import UIKit

struct LiveActivityView: View {
  @Environment(GooseAppModel.self) private var model

  var body: some View {
    LiveActivityContentView(
      ble: model.ble,
      session: model.activitySession,
      locationTracker: model.activityLocationTracker
    )
    .environment(model)
  }
}

