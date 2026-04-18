use serde::Deserialize;
use yew::prelude::*;

use crate::wiki::WikiBrowser;

#[derive(Clone, Debug, Deserialize, PartialEq)]
struct AgentInfo {
    id: String,
    name: String,
    role: String,
}

#[derive(Clone, PartialEq)]
enum ViewMode {
    Agents,
    Wiki,
}

#[function_component(App)]
pub fn app() -> Html {
    let agents = use_state(Vec::<AgentInfo>::new);
    let selected = use_state(|| None::<String>);
    let view = use_state(|| ViewMode::Agents);

    // Fetch agent list on mount.
    {
        let agents = agents.clone();
        use_effect_with((), move |_| {
            fetch_agents(agents);
        });
    }

    let on_select = {
        let selected = selected.clone();
        Callback::from(move |id: String| {
            selected.set(Some(id));
        })
    };

    let on_view_agents = {
        let view = view.clone();
        Callback::from(move |_: MouseEvent| view.set(ViewMode::Agents))
    };

    let on_view_wiki = {
        let view = view.clone();
        Callback::from(move |_: MouseEvent| view.set(ViewMode::Wiki))
    };

    let current_agent = (*selected)
        .clone()
        .or_else(|| agents.first().map(|a| a.id.clone()));

    let on_new_agent = Callback::from(|_: MouseEvent| new_agent_clicked());

    html! {
        <div class="app">
            <header>
                <h1>{ "All Together Now" }</h1>
                <span class="agent-count">
                    { format!("{} agent{}", agents.len(), if agents.len() != 1 { "s" } else { "" }) }
                </span>
                <button class="btn-new-agent" onclick={on_new_agent.clone()}>
                    { "+ New Agent" }
                </button>
                <div class="view-tabs">
                    <button
                        class={classes!("view-tab", (*view == ViewMode::Agents).then_some("active"))}
                        onclick={on_view_agents}
                    >{ "Agents" }</button>
                    <button
                        class={classes!("view-tab", (*view == ViewMode::Wiki).then_some("active"))}
                        onclick={on_view_wiki}
                    >{ "Wiki" }</button>
                </div>
            </header>
            if *view == ViewMode::Agents {
                if agents.is_empty() {
                    <EmptyAgentsState on_new_agent={on_new_agent} />
                } else {
                    <nav class="agent-tabs">
                        { for agents.iter().map(|a| {
                            let id = a.id.clone();
                            let is_active = current_agent.as_deref() == Some(&a.id);
                            let on_click = {
                                let on_select = on_select.clone();
                                let id = id.clone();
                                Callback::from(move |_: MouseEvent| on_select.emit(id.clone()))
                            };
                            html! {
                                <button
                                    class={classes!("tab", is_active.then_some("active"))}
                                    onclick={on_click}
                                >
                                    { &a.name }
                                    <span class="tab-role">{ &a.role }</span>
                                </button>
                            }
                        })}
                    </nav>
                    if let Some(agent_id) = current_agent {
                        <AgentPanel id={agent_id} />
                    }
                }
            } else {
                <WikiBrowser />
            }
        </div>
    }
}

#[derive(Properties, PartialEq)]
struct EmptyAgentsStateProps {
    on_new_agent: Callback<MouseEvent>,
}

#[function_component(EmptyAgentsState)]
fn empty_agents_state(props: &EmptyAgentsStateProps) -> Html {
    html! {
        <div class="empty-state">
            <h2>{ "No agents yet" }</h2>
            <p>
                { "ATN is running but no agents are configured. Add one to start—\
                   pick a host (local or remote), working directory, and the agent CLI to run." }
            </p>
            <button class="btn-new-agent-primary" onclick={props.on_new_agent.clone()}>
                { "+ New Agent" }
            </button>
        </div>
    }
}

#[derive(Properties, PartialEq)]
struct AgentPanelProps {
    id: String,
}

#[function_component(AgentPanel)]
fn agent_panel(props: &AgentPanelProps) -> Html {
    let input_ref = use_node_ref();
    let input_value = use_state(String::new);
    let agent_id = props.id.clone();

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
        let agent_id = agent_id.clone();
        Callback::from(move |_: MouseEvent| {
            let text = (*input_value).clone();
            if !text.is_empty() {
                send_input(&agent_id, &text);
                input_value.set(String::new());
                if let Some(el) = input_ref.cast::<web_sys::HtmlInputElement>() {
                    let _ = el.focus();
                }
            }
        })
    };

    let on_keydown = {
        let input_value = input_value.clone();
        let agent_id = agent_id.clone();
        Callback::from(move |e: KeyboardEvent| {
            if e.key() == "Enter" {
                let text = (*input_value).clone();
                if !text.is_empty() {
                    send_input(&agent_id, &text);
                    input_value.set(String::new());
                }
            }
        })
    };

    let on_ctrl_c = {
        let agent_id = agent_id.clone();
        Callback::from(move |_: MouseEvent| {
            send_ctrl_c(&agent_id);
        })
    };

    html! {
        <div class="panel">
            <div id={format!("terminal-{}", props.id)}></div>
            <div class="controls">
                <input
                    ref={input_ref}
                    type="text"
                    placeholder={format!("Command for {}...", props.id)}
                    value={(*input_value).clone()}
                    oninput={on_input}
                    onkeydown={on_keydown}
                />
                <button class="btn-send" onclick={on_send}>{ "Send" }</button>
                <button class="btn-ctrl-c" onclick={on_ctrl_c}>{ "^C" }</button>
            </div>
        </div>
    }
}

#[cfg(target_arch = "wasm32")]
fn fetch_agents(agents: UseStateHandle<Vec<AgentInfo>>) {
    use gloo_net::http::Request;
    wasm_bindgen_futures::spawn_local(async move {
        if let Ok(resp) = Request::get("/api/agents").send().await {
            if let Ok(list) = resp.json::<Vec<AgentInfo>>().await {
                agents.set(list);
            }
        }
    });
}

#[cfg(not(target_arch = "wasm32"))]
fn fetch_agents(_agents: UseStateHandle<Vec<AgentInfo>>) {}

#[cfg(target_arch = "wasm32")]
fn send_input(agent_id: &str, text: &str) {
    use gloo_net::http::Request;
    let url = format!("/api/agents/{}/input", agent_id);
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
fn send_input(_agent_id: &str, _text: &str) {}

#[cfg(target_arch = "wasm32")]
fn send_ctrl_c(agent_id: &str) {
    use gloo_net::http::Request;
    let url = format!("/api/agents/{}/ctrl-c", agent_id);
    wasm_bindgen_futures::spawn_local(async move {
        let _ = Request::post(&url).send().await;
    });
}

#[cfg(not(target_arch = "wasm32"))]
fn send_ctrl_c(_agent_id: &str) {}

#[cfg(target_arch = "wasm32")]
fn new_agent_clicked() {
    if let Some(win) = web_sys::window() {
        let _ = win.alert_with_message(
            "New Agent dialog arrives in the next step. For now, use the + Agent \
             form on the static dashboard at /index.html.",
        );
    }
}

#[cfg(not(target_arch = "wasm32"))]
fn new_agent_clicked() {}
