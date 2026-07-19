// UNUserNotificationCenter delegate (plan §7 step 2): handles the text-input
// reply from a check-in notification. The reply handler (SessionCoordinator)
// stores the entry unconditionally before attempting inference.

import Foundation
import UserNotifications

final class NotificationDelegate: NSObject, UNUserNotificationCenterDelegate {
    /// Wired by AppEnvironment to SessionCoordinator.handleNotificationReply.
    var onReply: ((String) -> Void)?

    func userNotificationCenter(_ center: UNUserNotificationCenter,
                                didReceive response: UNNotificationResponse,
                                withCompletionHandler completionHandler: @escaping () -> Void) {
        if let textResponse = response as? UNTextInputNotificationResponse,
           response.actionIdentifier == NotificationScheduler.replyActionId {
            onReply?(textResponse.userText)
        }
        // Default action (tap) just opens the app; foregrounding reschedules.
        completionHandler()
    }

    func userNotificationCenter(_ center: UNUserNotificationCenter,
                                willPresent notification: UNNotification,
                                withCompletionHandler completionHandler:
                                @escaping (UNNotificationPresentationOptions) -> Void) {
        completionHandler([.banner, .sound])
    }
}
