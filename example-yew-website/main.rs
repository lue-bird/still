#![feature(deref_patterns)]
#![allow(incomplete_features, non_shorthand_field_patterns)]
mod still;
fn main() {
    yew::Renderer::<App>::new().render();
}

pub struct App {
    still_state: still::State,
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
            still_state: still::initial_state(),
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
                let current_interface = still::view(self.still_state.clone());

                let maybe_target_modifiers = match still_virtual_dom_lookup_dom_node_at_path(
                    &current_interface,
                    fire_target_dom_path.into_iter(),
                ) {
                    Option::None => Option::None,
                    Option::Some(dom_node_at_path) => match dom_node_at_path {
                        still::Html::Text(_) => Option::None,
                        still::Html::Element(element) => Option::Some(element.modifiers.clone()),
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
                                still::Modifier::Listen(listen) => {
                                    if &listen.name == fired_event_name.as_str() {
                                        let new_still_event = {
                                            (listen.on)(web_sys_js_value_to_still_json(
                                                &web_sys_event,
                                            ))
                                        };
                                        self.still_state = still::update(
                                            new_still_event,
                                            self.still_state.clone(),
                                        );
                                        // uncomment to debug
                                        // web_sys::console::log_1(
                                        //     &web_sys::js_sys::JsString::from(format!(
                                        //         "still event {:?} → updated state {:?}",
                                        //         new_still_event, self.still_state
                                        //     )),
                                        // );
                                        break 'associating_element_modifier;
                                    }
                                }
                                still::Modifier::Attribute(_) => {}
                                still::Modifier::Style(_) => {}
                                still::Modifier::Property(_) => {}
                            }
                        }
                    }
                }
            }
        }
        true
    }

    fn view(&self, context: &yew::Context<Self>) -> yew::Html {
        still_dom_node_to_yew(
            context.link(),
            &mut Vec::new(),
            Option::None,
            &still::view(self.still_state.clone()),
        )
    }
}

fn still_dom_node_to_yew<Event>(
    yew_scope: &yew::html::Scope<App>,
    dom_path: &mut Vec<usize>,
    maybe_key: Option<&str>,
    still_dom_node: &still::Html<Event>,
) -> yew::Html {
    match still_dom_node {
        still::Html::Text(text) => yew::Html::VText(yew::virtual_dom::VText::from(text)),
        still::Html::Element(element) => {
            let mut vtag: yew::virtual_dom::VTag =
                yew::virtual_dom::VTag::new(element.tag.to_string());
            match maybe_key {
                Option::None => {}
                Option::Some(key) => vtag.key = Option::Some(yew::virtual_dom::Key::from(key)),
            }
            vtag.add_children(element.subs.iter().enumerate().map(|(sub_index, sub)| {
                dom_path.push(sub_index);
                let sub_yew_node = still_dom_node_to_yew(yew_scope, dom_path, Option::None, sub);
                dom_path.pop();
                sub_yew_node
            }));
            yew_vtag_add_still_virtual_dom_modifiers(
                yew_scope,
                dom_path,
                &mut vtag,
                element.modifiers.as_slice(),
            );
            yew::Html::VTag(Box::new(vtag))
        }
    }
}

fn yew_vtag_add_still_virtual_dom_modifiers<Event>(
    yew_scope: &yew::html::Scope<App>,
    dom_path: &[usize],
    yew_vtag: &mut yew::virtual_dom::VTag,
    still_virtual_dom_modifiers: &[still::Modifier<Event>],
) {
    let styles: Vec<String> = still_virtual_dom_modifiers
        .iter()
        .filter_map(|modifier| match modifier {
            still::Modifier::Style(style) => Option::Some(format!("{}:{}", style.key, style.value)),
            _ => Option::None,
        })
        .collect::<Vec<_>>();
    if !styles.is_empty() {
        yew_vtag.add_attribute("style", styles.join(";"));
    }
    for modifier in still_virtual_dom_modifiers.iter() {
        yew_vtag_add_still_virtual_dom_modifier_except_style(
            yew_scope, dom_path, yew_vtag, modifier,
        )
    }
}
fn yew_vtag_add_still_virtual_dom_modifier_except_style<Event>(
    yew_scope: &yew::html::Scope<App>,
    dom_path: &[usize],
    yew_vtag: &mut yew::virtual_dom::VTag,
    still_virtual_dom_modifier: &still::Modifier<Event>,
) {
    match still_virtual_dom_modifier {
        still::Modifier::Style(_) => {}
        still::Modifier::Attribute(attribute) => {
            yew_vtag.attributes.get_mut_index_map().insert(
                yew::AttrValue::from(attribute.key.to_string()),
                (
                    yew::AttrValue::from(attribute.value.to_string()),
                    yew::virtual_dom::ApplyAttributeAs::Attribute,
                ),
            );
        }
        still::Modifier::Property(property) => {
            yew_vtag.attributes.get_mut_index_map().insert(
                yew::AttrValue::from(property.key.to_string()),
                (
                    match &property.value {
                        still::Json::Null => yew::AttrValue::from("null"),
                        still::Json::True => yew::AttrValue::from("true"),
                        still::Json::False => yew::AttrValue::from("false"),
                        still::Json::Number(number) => yew::AttrValue::from(number.to_string()),
                        still::Json::String(str) => yew::AttrValue::from(str.to_string()),
                        still::Json::Array(_) => unimplemented!(),
                        still::Json::Object(_) => unimplemented!(),
                    },
                    yew::virtual_dom::ApplyAttributeAs::Property,
                ),
            );
        }
        still::Modifier::Listen(listen) => {
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

fn still_virtual_dom_lookup_dom_node_at_path<Event: Clone>(
    still_virtual_dom_node: &still::Html<Event>,
    mut path: impl Iterator<Item = usize>, // consider &mut iterator
) -> Option<&still::Html<Event>> {
    match path.next() {
        Option::None => Option::Some(still_virtual_dom_node),
        Option::Some(sub_index) => match still_virtual_dom_node {
            still::Html::Text(_) => Option::None,
            still::Html::Element(element) => match element.subs.as_slice().get(sub_index) {
                Option::None => Option::None,
                Option::Some(sub_node) => still_virtual_dom_lookup_dom_node_at_path(sub_node, path),
            },
        },
    }
}
fn web_sys_js_value_to_still_json(
    web_sys_js_value: &web_sys::wasm_bindgen::JsValue,
) -> still::Json {
    // shows that still::JsonValue should probably be lazy internally for full array and object
    if web_sys_js_value.is_null() {
        still::Json::Null
    } else {
        match web_sys_js_value.as_bool() {
            Option::Some(false) => still::Json::False,
            Option::Some(true) => still::Json::True,
            Option::None => {
                match web_sys_js_value.as_f64() {
                    Option::Some(number) => still::Json::Number(number),
                    Option::None => {
                        match web_sys_js_value.as_string() {
                            Option::Some(string) => {
                                still::Json::String(still::Str::from_string(string))
                            }
                            Option::None => {
                                if web_sys_js_value.is_array() {
                                    still::Json::Array(std::rc::Rc::new(still::Vec::from_vec(
                                        web_sys::js_sys::Array::from(web_sys_js_value)
                                            .iter()
                                            .map(|element| web_sys_js_value_to_still_json(&element))
                                            .collect::<Vec<_>>(),
                                    )))
                                } else {
                                    match web_sys::js_sys::Object::try_from(web_sys_js_value) {
                                        Option::Some(js_object) => {
                                            still::Json::Object(std::rc::Rc::new(
                                                still::Vec::from_vec(
                                                    web_sys::js_sys::Object::keys(
                                                        &web_sys::js_sys::Object::get_prototype_of(
                                                            js_object,
                                                        ),
                                                    )
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
                                                        let maybe_key = key.as_string();
                                                        let maybe_value =
                                                            web_sys::js_sys::Reflect::get(
                                                                js_object, &key,
                                                            )
                                                            .ok();
                                                        maybe_key.zip(maybe_value).map(
                                                            |(key, value)| still::Key·value {
                                                                key: still::Str::from_string(key),
                                                                value:
                                                                    web_sys_js_value_to_still_json(
                                                                        &value,
                                                                    ),
                                                            },
                                                        )
                                                    })
                                                    .collect::<Vec<_>>(),
                                                ),
                                            ))
                                        }
                                        Option::None => {
                                            // maybe cleaner to return Option::None
                                            still::Json::Null
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }
    }
}
