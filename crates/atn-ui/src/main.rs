#[cfg(target_arch = "wasm32")]
fn main() {
    yew::Renderer::<atn_ui::App>::new().render();
}

#[cfg(not(target_arch = "wasm32"))]
fn main() {
    // Native stub — use `trunk serve` to build and serve the WASM app.
    eprintln!("atn-ui is a WASM application. Use `trunk serve` from crates/atn-ui/");
}
