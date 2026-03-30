use yew::prelude::*;

#[function_component(App)]
pub fn app() -> Html {
    let input_ref = use_node_ref();
    let input_value = use_state(String::new);

    let on_input = {
        let input_value = input_value.clone();
        Callback::from(move |e: InputEvent| {
            let input: web_sys::HtmlInputElement = e.target_unchecked_into();
            input_value.set(input.value());
        })
    };

    let on_send = {
        let input_value = input_value.clone();
        let input_ref = input_ref.clone();
        Callback::from(move |_: MouseEvent| {
            let text = (*input_value).clone();
            if !text.is_empty() {
                send_input(&text);
                input_value.set(String::new());
                if let Some(el) = input_ref.cast::<web_sys::HtmlInputElement>() {
                    let _ = el.focus();
                }
            }
        })
    };

    let on_keydown = {
        let input_value = input_value.clone();
        Callback::from(move |e: KeyboardEvent| {
            if e.key() == "Enter" {
                let text = (*input_value).clone();
                if !text.is_empty() {
                    send_input(&text);
                    input_value.set(String::new());
                }
            }
        })
    };

    let on_ctrl_c = Callback::from(|_: MouseEvent| {
        send_ctrl_c();
    });

    html! {
        <div class="app">
            <header>
                <h1>{ "All Together Now" }</h1>
                <span class="agent-label">{ "demo" }</span>
                <span class="status" id="agent-status">{ "connecting..." }</span>
            </header>
            <div class="panel">
                <div id="terminal-container"></div>
                <div class="controls">
                    <input
                        ref={input_ref}
                        type="text"
                        id="input-box"
                        placeholder="Type a command..."
                        value={(*input_value).clone()}
                        oninput={on_input}
                        onkeydown={on_keydown}
                    />
                    <button class="btn-send" onclick={on_send}>{ "Send" }</button>
                    <button class="btn-ctrl-c" onclick={on_ctrl_c}>{ "Ctrl-C" }</button>
                </div>
            </div>
        </div>
    }
}

#[cfg(target_arch = "wasm32")]
fn send_input(text: &str) {
    use gloo_net::http::Request;
    let url = format!("/api/agents/demo/input");
    let body = serde_json::json!({ "text": text }).to_string();
    wasm_bindgen_futures::spawn_local(async move {
        let _ = Request::post(&url)
            .header("Content-Type", "application/json")
            .body(body)
            .unwrap()
            .send()
            .await;
    });
}

#[cfg(not(target_arch = "wasm32"))]
fn send_input(_text: &str) {}

#[cfg(target_arch = "wasm32")]
fn send_ctrl_c() {
    use gloo_net::http::Request;
    wasm_bindgen_futures::spawn_local(async move {
        let _ = Request::post("/api/agents/demo/ctrl-c")
            .send()
            .await;
    });
}

#[cfg(not(target_arch = "wasm32"))]
fn send_ctrl_c() {}
