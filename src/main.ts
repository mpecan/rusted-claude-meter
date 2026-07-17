// Entry point for the popover/settings UI. The scaffold renders a static
// placeholder; usage cards are driven by state pushed from Rust via events
// once the polling scheduler exists.

window.addEventListener("DOMContentLoaded", () => {
  const status = document.querySelector<HTMLElement>("#status");
  if (status) {
    status.dataset.ready = "true";
  }
});
