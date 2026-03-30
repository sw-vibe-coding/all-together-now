use serde::Deserialize;
use yew::prelude::*;

#[derive(Clone, Debug, Deserialize, PartialEq)]
struct WikiPageData {
    title: String,
    content: String,
    html: String,
    #[allow(dead_code)]
    created_at: u64,
    #[allow(dead_code)]
    updated_at: u64,
}

#[function_component(WikiBrowser)]
pub fn wiki_browser() -> Html {
    let pages = use_state(Vec::<String>::new);
    let current_page = use_state(|| None::<WikiPageData>);
    let editing = use_state(|| false);
    let editor_content = use_state(String::new);
    let etag = use_state(|| None::<String>);
    let conflict_msg = use_state(|| None::<String>);
    let new_title = use_state(String::new);

    // Fetch page list on mount.
    {
        let pages = pages.clone();
        use_effect_with((), move |_| {
            fetch_page_list(pages);
        });
    }

    let on_select_page = {
        let current_page = current_page.clone();
        let etag = etag.clone();
        let editing = editing.clone();
        let editor_content = editor_content.clone();
        let conflict_msg = conflict_msg.clone();
        let pages = pages.clone();
        Callback::from(move |title: String| {
            editing.set(false);
            conflict_msg.set(None);
            let current_page = current_page.clone();
            let etag = etag.clone();
            let editor_content = editor_content.clone();
            let pages = pages.clone();
            fetch_page(title, current_page, etag, editor_content, pages);
        })
    };

    let on_edit = {
        let editing = editing.clone();
        Callback::from(move |_: MouseEvent| {
            editing.set(true);
        })
    };

    let on_cancel = {
        let editing = editing.clone();
        let conflict_msg = conflict_msg.clone();
        let current_page = current_page.clone();
        let editor_content = editor_content.clone();
        Callback::from(move |_: MouseEvent| {
            editing.set(false);
            conflict_msg.set(None);
            if let Some(ref page) = *current_page {
                editor_content.set(page.content.clone());
            }
        })
    };

    let on_editor_input = {
        let editor_content = editor_content.clone();
        Callback::from(move |e: InputEvent| {
            let ta: web_sys::HtmlTextAreaElement = e.target_unchecked_into();
            editor_content.set(ta.value());
        })
    };

    let on_save = {
        let current_page = current_page.clone();
        let editor_content = editor_content.clone();
        let etag = etag.clone();
        let editing = editing.clone();
        let conflict_msg = conflict_msg.clone();
        let pages = pages.clone();
        Callback::from(move |_: MouseEvent| {
            if let Some(ref page) = *current_page {
                let title = page.title.clone();
                let content = (*editor_content).clone();
                let etag_val = (*etag).clone();
                let current_page = current_page.clone();
                let etag = etag.clone();
                let editor_content = editor_content.clone();
                let editing = editing.clone();
                let conflict_msg = conflict_msg.clone();
                let pages = pages.clone();
                save_page(
                    title,
                    content,
                    etag_val,
                    current_page,
                    etag,
                    editor_content,
                    editing,
                    conflict_msg,
                    pages,
                );
            }
        })
    };

    let on_new_title_input = {
        let new_title = new_title.clone();
        Callback::from(move |e: InputEvent| {
            let input: web_sys::HtmlInputElement = e.target_unchecked_into();
            new_title.set(input.value());
        })
    };

    let on_new_page = {
        let new_title = new_title.clone();
        let on_select_page = on_select_page.clone();
        Callback::from(move |_: MouseEvent| {
            let title = (*new_title).clone().trim().to_string();
            if !title.is_empty() {
                new_title.set(String::new());
                let on_select_page = on_select_page.clone();
                create_page(title.clone(), on_select_page);
            }
        })
    };

    let on_wiki_link = {
        let on_select_page = on_select_page.clone();
        Callback::from(move |title: String| {
            on_select_page.emit(title);
        })
    };

    let current_title = (*current_page).as_ref().map(|p| p.title.clone());

    html! {
        <div class="wiki-container">
            <div class="wiki-sidebar">
                <h3>{ "Pages" }</h3>
                <div class="wiki-page-list">
                    { for (*pages).iter().map(|title| {
                        let is_active = current_title.as_deref() == Some(title.as_str());
                        let on_click = {
                            let on_select_page = on_select_page.clone();
                            let title = title.clone();
                            Callback::from(move |_: MouseEvent| on_select_page.emit(title.clone()))
                        };
                        html! {
                            <div
                                class={classes!("wiki-page-item", is_active.then_some("active"))}
                                onclick={on_click}
                            >
                                { title.replace("__", "/") }
                            </div>
                        }
                    })}
                </div>
                <div class="wiki-new-page">
                    <input
                        type="text"
                        placeholder="New page..."
                        value={(*new_title).clone()}
                        oninput={on_new_title_input}
                    />
                    <button onclick={on_new_page}>{ "+" }</button>
                </div>
            </div>
            <div class="wiki-main">
                <div class="wiki-toolbar">
                    <span class="page-title">
                        { current_title.as_deref().unwrap_or("Select a page") }
                    </span>
                    if current_page.is_some() && !*editing {
                        <button onclick={on_edit}>{ "Edit" }</button>
                    }
                    if *editing {
                        <button class="btn-save" onclick={on_save}>{ "Save" }</button>
                        <button onclick={on_cancel}>{ "Cancel" }</button>
                    }
                </div>
                if let Some(ref msg) = *conflict_msg {
                    <div class="wiki-conflict">{ msg }</div>
                }
                if *editing {
                    <div class="wiki-edit-area">
                        <textarea
                            value={(*editor_content).clone()}
                            oninput={on_editor_input}
                        />
                    </div>
                } else if let Some(ref page) = *current_page {
                    <WikiContentView html={page.html.clone()} on_link={on_wiki_link} />
                }
            </div>
        </div>
    }
}

#[derive(Properties, PartialEq)]
struct WikiContentProps {
    html: String,
    on_link: Callback<String>,
}

#[function_component(WikiContentView)]
fn wiki_content_view(props: &WikiContentProps) -> Html {
    let div_ref = use_node_ref();
    let html_content = props.html.clone();
    let on_link = props.on_link.clone();

    // Set innerHTML and wire up wiki-link clicks.
    {
        let div_ref = div_ref.clone();
        use_effect_with(html_content.clone(), move |html| {
            if let Some(el) = div_ref.cast::<web_sys::Element>() {
                el.set_inner_html(html);
                wire_wiki_links(&el, &on_link);
            }
        });
    }

    html! {
        <div class="wiki-content" ref={div_ref} />
    }
}

#[cfg(target_arch = "wasm32")]
fn wire_wiki_links(el: &web_sys::Element, on_link: &Callback<String>) {
    use wasm_bindgen::prelude::*;

    let links = el.query_selector_all("a.wiki-link").unwrap();
    for i in 0..links.length() {
        let node = links.item(i).unwrap();
        let anchor: web_sys::HtmlElement = node.unchecked_into();
        if let Some(target) = anchor.get_attribute("data-wiki-link") {
            let cb = on_link.clone();
            let closure = Closure::wrap(Box::new(move |e: web_sys::Event| {
                e.prevent_default();
                cb.emit(target.clone());
            }) as Box<dyn Fn(_)>);
            anchor
                .add_event_listener_with_callback("click", closure.as_ref().unchecked_ref())
                .unwrap();
            closure.forget(); // leaked — acceptable for UI event handlers
        }
    }
}

#[cfg(not(target_arch = "wasm32"))]
fn wire_wiki_links(_el: &web_sys::Element, _on_link: &Callback<String>) {}

// ── WASM async helpers ─────────────────────────────────────────────────

#[cfg(target_arch = "wasm32")]
fn fetch_page_list(pages: UseStateHandle<Vec<String>>) {
    use gloo_net::http::Request;
    wasm_bindgen_futures::spawn_local(async move {
        if let Ok(resp) = Request::get("/api/wiki").send().await {
            if let Ok(list) = resp.json::<Vec<String>>().await {
                pages.set(list);
            }
        }
    });
}

#[cfg(not(target_arch = "wasm32"))]
fn fetch_page_list(_pages: UseStateHandle<Vec<String>>) {}

#[cfg(target_arch = "wasm32")]
fn fetch_page(
    title: String,
    current_page: UseStateHandle<Option<WikiPageData>>,
    etag: UseStateHandle<Option<String>>,
    editor_content: UseStateHandle<String>,
    pages: UseStateHandle<Vec<String>>,
) {
    use gloo_net::http::Request;
    wasm_bindgen_futures::spawn_local(async move {
        let url = format!("/api/wiki/{}", js_sys::encode_uri_component(&title));
        if let Ok(resp) = Request::get(&url).send().await {
            if let Ok(data) = resp.json::<WikiPageData>().await {
                editor_content.set(data.content.clone());
                etag.set(
                    resp.headers()
                        .get("ETag")
                        .or_else(|| resp.headers().get("etag")),
                );
                current_page.set(Some(data));
            }
        }
        // Refresh page list.
        if let Ok(resp) = Request::get("/api/wiki").send().await {
            if let Ok(list) = resp.json::<Vec<String>>().await {
                pages.set(list);
            }
        }
    });
}

#[cfg(not(target_arch = "wasm32"))]
fn fetch_page(
    _title: String,
    _current_page: UseStateHandle<Option<WikiPageData>>,
    _etag: UseStateHandle<Option<String>>,
    _editor_content: UseStateHandle<String>,
    _pages: UseStateHandle<Vec<String>>,
) {
}

#[cfg(target_arch = "wasm32")]
#[allow(clippy::too_many_arguments)]
fn save_page(
    title: String,
    content: String,
    etag_val: Option<String>,
    current_page: UseStateHandle<Option<WikiPageData>>,
    etag: UseStateHandle<Option<String>>,
    editor_content: UseStateHandle<String>,
    editing: UseStateHandle<bool>,
    conflict_msg: UseStateHandle<Option<String>>,
    pages: UseStateHandle<Vec<String>>,
) {
    use gloo_net::http::Request;
    wasm_bindgen_futures::spawn_local(async move {
        let url = format!("/api/wiki/{}", js_sys::encode_uri_component(&title));
        let mut req = Request::put(&url)
            .header("Content-Type", "application/json");
        if let Some(ref etag_str) = etag_val {
            req = req.header("If-Match", etag_str);
        }
        let body = serde_json::json!({ "content": content }).to_string();
        if let Ok(resp) = req.body(body).unwrap().send().await {
            if resp.status() == 409 {
                conflict_msg.set(Some(
                    "Conflict: page was modified. Reload and try again.".to_string(),
                ));
                return;
            }
            if resp.ok() {
                conflict_msg.set(None);
                editing.set(false);
                // Re-fetch the page to get updated html/etag.
                fetch_page(title, current_page, etag, editor_content, pages);
            }
        }
    });
}

#[cfg(not(target_arch = "wasm32"))]
#[allow(clippy::too_many_arguments)]
fn save_page(
    _title: String,
    _content: String,
    _etag_val: Option<String>,
    _current_page: UseStateHandle<Option<WikiPageData>>,
    _etag: UseStateHandle<Option<String>>,
    _editor_content: UseStateHandle<String>,
    _editing: UseStateHandle<bool>,
    _conflict_msg: UseStateHandle<Option<String>>,
    _pages: UseStateHandle<Vec<String>>,
) {
}

#[cfg(target_arch = "wasm32")]
fn create_page(title: String, on_done: Callback<String>) {
    use gloo_net::http::Request;
    wasm_bindgen_futures::spawn_local(async move {
        let url = format!("/api/wiki/{}", js_sys::encode_uri_component(&title));
        let body = serde_json::json!({ "content": format!("# {}\n\n", title) }).to_string();
        if let Ok(resp) = Request::put(&url)
            .header("Content-Type", "application/json")
            .body(body)
            .unwrap()
            .send()
            .await
        {
            if resp.ok() || resp.status() == 201 {
                on_done.emit(title);
            }
        }
    });
}

#[cfg(not(target_arch = "wasm32"))]
fn create_page(_title: String, _on_done: Callback<String>) {}
