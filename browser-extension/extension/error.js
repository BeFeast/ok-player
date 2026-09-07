const params = new URLSearchParams(location.search);
const code = params.get("code") || "helper_error";
const suppliedMessage = params.get("message");

const actions = {
  helper_unavailable: "Install the helper with browser-extension/install.py --apply, then retry.",
  host_not_configured: "Reinstall the browser integration with the absolute OK Player executable path.",
  player_not_found: "Reinstall with --player set to an existing executable, such as /home/YOU/.local/bin/ok-player.",
  player_not_executable: "Make the configured OK Player launcher executable or reinstall with the correct --player path.",
  launch_failed: "Check the installed OK Player launcher, then reinstall the browser integration if its path changed.",
  unsupported_url: "Open an HTTP or HTTPS video link or page instead.",
  unsupported_video: "Open the video's HTTP or HTTPS page instead of its temporary source.",
  unsafe_url: "Use the original HTTP or HTTPS URL without leading options, whitespace, or control characters.",
  missing_url: "Try the menu on a video link, embedded video, or ordinary web page.",
};

document.querySelector("#message").textContent =
  suppliedMessage || "The browser integration could not handle this request.";
document.querySelector("#action").textContent =
  actions[code] || "Reinstall the OK Player browser integration, then retry.";
