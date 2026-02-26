#![feature(deref_patterns)]
#![allow(incomplete_features, non_shorthand_field_patterns)]
mod lily;
fn main() {
    yew::Renderer::<App>::new().render();
}

pub struct App {
    lily_state: lily::State,
}
pub enum AppEvent {
    DomEventFired {
        name: String,
        dom_path: Vec<usize>,
        web_sys_event: web_sys::Event,
    },
}
impl yew::Component for App {
    type Message = AppEvent;

    type Properties = ();

    fn create(_context: &yew::Context<Self>) -> Self {
        App {
            lily_state: lily::initial_state(),
        }
    }

    fn update(&mut self, _context: &yew::Context<Self>, event: Self::Message) -> bool {
        match event {
            AppEvent::DomEventFired {
                name: fired_event_name,
                dom_path: fire_target_dom_path,
                web_sys_event: web_sys_event,
            } => {
                // lookup same dom path
                // then call its event handler
                // with the then-created event, create the new state
                let current_interface = lily::view(self.lily_state.clone());

                let maybe_target_modifiers = match lily_virtual_dom_lookup_dom_node_at_path(
                    &current_interface,
                    fire_target_dom_path.into_iter(),
                ) {
                    Option::None => Option::None,
                    Option::Some(dom_node_at_path) => match dom_node_at_path {
                        lily::Html::Text(_) => Option::None,
                        lily::Html::Element(element) => Option::Some(element.modifiers.clone()),
                    },
                };
                match maybe_target_modifiers {
                    Option::None => {
                        // lookup _can_ correctly return None sometimes
                        // when it is de-synced. Is expected when rapid-firing events
                        // and can be safely ignored
                        println!("failed to associate dom target")
                    }
                    Option::Some(target_modifiers) => {
                        'associating_element_modifier: for modifier in target_modifiers.iter() {
                            match modifier {
                                lily::Modifier::Listen(listen) => {
                                    if &listen.name == fired_event_name.as_str() {
                                        let new_lily_event = {
                                            (listen.on)(web_sys_js_value_to_lily_json(
                                                &web_sys_event,
                                            ))
                                        };
                                        self.lily_state = lily::update(
                                            new_lily_event,
                                            self.lily_state.clone(),
                                        );
                                        // uncomment to debug
                                        // web_sys::console::log_1(
                                        //     &web_sys::js_sys::JsString::from(format!(
                                        //         "lily event {:?} → updated state {:?}",
                                        //         new_lily_event, self.lily_state
                                        //     )),
                                        // );
                                        break 'associating_element_modifier;
                                    }
                                }
                                lily::Modifier::Attribute(_) => {}
                                lily::Modifier::Style(_) => {}
                                lily::Modifier::Property(_) => {}
                            }
                        }
                    }
                }
            }
        }
        true
    }

    fn view(&self, context: &yew::Context<Self>) -> yew::Html {
        lily_dom_node_to_yew(
            context.link(),
            &mut Vec::new(),
            Option::None,
            &lily::view(self.lily_state.clone()),
        )
    }
}

fn lily_dom_node_to_yew<Event>(
    yew_scope: &yew::html::Scope<App>,
    dom_path: &mut Vec<usize>,
    maybe_key: Option<&str>,
    lily_dom_node: &lily::Html<Event>,
) -> yew::Html {
    match lily_dom_node {
        lily::Html::Text(text) => yew::Html::VText(yew::virtual_dom::VText::from(text)),
        lily::Html::Element(element) => {
            let mut vtag: yew::virtual_dom::VTag =
                yew::virtual_dom::VTag::new(element.tag.to_string());
            match maybe_key {
                Option::None => {}
                Option::Some(key) => vtag.key = Option::Some(yew::virtual_dom::Key::from(key)),
            }
            vtag.add_children(element.subs.iter().enumerate().map(|(sub_index, sub)| {
                dom_path.push(sub_index);
                let sub_yew_node = lily_dom_node_to_yew(yew_scope, dom_path, Option::None, sub);
                dom_path.pop();
                sub_yew_node
            }));
            yew_vtag_add_lily_virtual_dom_modifiers(
                yew_scope,
                dom_path,
                &mut vtag,
                element.modifiers.as_slice(),
            );
            yew::Html::VTag(Box::new(vtag))
        }
    }
}

fn yew_vtag_add_lily_virtual_dom_modifiers<Event>(
    yew_scope: &yew::html::Scope<App>,
    dom_path: &[usize],
    yew_vtag: &mut yew::virtual_dom::VTag,
    lily_virtual_dom_modifiers: &[lily::Modifier<Event>],
) {
    let styles: Vec<String> = lily_virtual_dom_modifiers
        .iter()
        .filter_map(|modifier| match modifier {
            lily::Modifier::Style(style) => Option::Some(format!("{}:{}", style.key, style.value)),
            _ => Option::None,
        })
        .collect::<Vec<_>>();
    if !styles.is_empty() {
        yew_vtag.add_attribute("style", styles.join(";"));
    }
    for modifier in lily_virtual_dom_modifiers.iter() {
        yew_vtag_add_lily_virtual_dom_modifier_except_style(
            yew_scope, dom_path, yew_vtag, modifier,
        )
    }
}
fn yew_vtag_add_lily_virtual_dom_modifier_except_style<Event>(
    yew_scope: &yew::html::Scope<App>,
    dom_path: &[usize],
    yew_vtag: &mut yew::virtual_dom::VTag,
    lily_virtual_dom_modifier: &lily::Modifier<Event>,
) {
    match lily_virtual_dom_modifier {
        lily::Modifier::Style(_) => {}
        lily::Modifier::Attribute(attribute) => {
            yew_vtag.attributes.get_mut_index_map().insert(
                yew::AttrValue::from(attribute.key.to_string()),
                (
                    yew::AttrValue::from(attribute.value.to_string()),
                    yew::virtual_dom::ApplyAttributeAs::Attribute,
                ),
            );
        }
        lily::Modifier::Property(property) => {
            yew_vtag.attributes.get_mut_index_map().insert(
                yew::AttrValue::from(property.key.to_string()),
                (
                    match &property.value {
                        lily::Json::Null => yew::AttrValue::from("null"),
                        lily::Json::True => yew::AttrValue::from("true"),
                        lily::Json::False => yew::AttrValue::from("false"),
                        lily::Json::Number(number) => yew::AttrValue::from(number.to_string()),
                        lily::Json::String(str) => yew::AttrValue::from(str.to_string()),
                        lily::Json::Array(_) => unimplemented!(),
                        lily::Json::Object(_) => unimplemented!(),
                    },
                    yew::virtual_dom::ApplyAttributeAs::Property,
                ),
            );
        }
        lily::Modifier::Listen(listen) => {
            let listener: std::rc::Rc<dyn yew::virtual_dom::Listener> =
                std::rc::Rc::new(YewRegisteredEventListener {
                    name: listen.name.to_string(),
                    dom_path: dom_path.to_vec(),
                    yew_scope: yew_scope.clone(),
                });
            yew_vtag.add_listener(listener);
        }
    }
}
struct YewRegisteredEventListener {
    name: String,
    dom_path: Vec<usize>,
    yew_scope: yew::html::Scope<App>,
}

impl yew::virtual_dom::Listener for YewRegisteredEventListener {
    fn kind(&self) -> yew::virtual_dom::ListenerKind {
        yew::virtual_dom::ListenerKind::other(std::borrow::Cow::Owned(self.name.clone()))
    }
    fn handle(&self, event: web_sys::Event) {
        let name_owned = self.name.clone();
        let dom_path_owned = self.dom_path.clone();
        self.yew_scope
            .callback(move |()| AppEvent::DomEventFired {
                dom_path: dom_path_owned.clone(),
                name: name_owned.clone(),
                web_sys_event: event.clone(),
            })
            .emit(());
    }
    fn passive(&self) -> bool {
        true
    }
}

fn lily_virtual_dom_lookup_dom_node_at_path<Event: Clone>(
    lily_virtual_dom_node: &lily::Html<Event>,
    mut path: impl Iterator<Item = usize>, // consider &mut iterator
) -> Option<&lily::Html<Event>> {
    match path.next() {
        Option::None => Option::Some(lily_virtual_dom_node),
        Option::Some(sub_index) => match lily_virtual_dom_node {
            lily::Html::Text(_) => Option::None,
            lily::Html::Element(element) => match element.subs.as_slice().get(sub_index) {
                Option::None => Option::None,
                Option::Some(sub_node) => lily_virtual_dom_lookup_dom_node_at_path(sub_node, path),
            },
        },
    }
}
fn web_sys_js_value_to_lily_json(
    web_sys_js_value: &web_sys::wasm_bindgen::JsValue,
) -> lily::Json {
    // shows that lily::JsonValue should probably be lazy internally for full array and object
    if web_sys_js_value.is_null() {
        return lily::Json::Null;
    }
    if let Option::Some(bool) = web_sys_js_value.as_bool() {
        return match bool {
            true => lily::Json::True,
            false => lily::Json::False,
        };
    }
    if let Option::Some(number) = web_sys_js_value.as_f64() {
        return lily::Json::Number(number);
    }
    if let Option::Some(string) = web_sys_js_value.as_string() {
        return lily::Json::String(lily::Str::from_string(string));
    }
    if web_sys_js_value.is_array() {
        return lily::Json::Array(std::rc::Rc::new(lily::Vec::from_vec(
            web_sys::js_sys::Array::from(web_sys_js_value)
                .iter()
                .map(|element| web_sys_js_value_to_lily_json(&element))
                .collect::<Vec<_>>(),
        )));
    }
    if let Option::Some(js_object) = web_sys::js_sys::Object::try_from(web_sys_js_value) {
        return lily::Json::Object(std::rc::Rc::new(lily::Vec::from_vec(
            web_sys::js_sys::Object::keys(&web_sys::js_sys::Object::get_prototype_of(js_object))
                // sanity check: all these do _not_ work:
                // Object::entries or
                // Object::keys or
                // Reflect::own_keys or
                // Reflect::apply(
                //     &Function::from(eval("(function(o) { return Object.keys(o); })")?,
                //     &global(),
                //     &Array::from_iter(std::iter::once(js_object))
                // ) or
                // JSON::stringify
                // and even when trying to remove proxies with
                // Object.assign({}, _)
                // They return a weird
                // { __yew_subtree_cache_key, __yew_subtree_id, trusted }
                // I tried a whole bunch of stuff but couldn't work out
                // why this happens (it only doesn't with console.log and getPrototypeOf ??).
                // If you know more, PLEASE tell me :)
                .into_iter()
                .filter_map(|key| {
                    let maybe_key: Option<String> = key.as_string();
                    let maybe_value: Option<web_sys::wasm_bindgen::JsValue> =
                        web_sys::js_sys::Reflect::get(js_object, &key).ok();
                    maybe_key
                        .zip(maybe_value)
                        .map(|(key, value)| lily::Key·value {
                            key: lily::Str::from_string(key),
                            value: web_sys_js_value_to_lily_json(&value),
                        })
                })
                .collect::<Vec<_>>(),
        )));
    }
    lily::Json::Null
}
