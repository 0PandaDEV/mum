/// Try to initialize the notification daemon.
///
/// Does nothing if compiled without notification support.
pub fn init() {
    #[cfg(feature = "notifications")]
    if let Err(e) = libnotify::init("mumd") {
        log::warn!("Unable to initialize notifications: {}", e);
    }
}

/// Send a notification. Without the `notifications`-feature, this will never do
/// anything and always return None.
pub fn send(_: String) -> Option<std::thread::JoinHandle<bool>> {
    None
}
