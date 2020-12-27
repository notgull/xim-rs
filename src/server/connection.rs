mod im_vec;

use ahash::AHashMap;
use std::num::{NonZeroU16, NonZeroU32};
use xim_parser::{
    bstr::{BStr, BString},
    Attr, AttrType, Attribute, AttributeName, ErrorCode, ErrorFlag, ForwardEventFlag, InputStyle,
    InputStyleList, Point, Request, XimWrite,
};

use self::im_vec::ImVec;
use crate::server::{Server, ServerCore, ServerError, ServerHandler};

pub struct InputContext<T> {
    client_win: u32,
    app_win: Option<NonZeroU32>,
    app_focus_win: Option<NonZeroU32>,
    input_method_id: NonZeroU16,
    input_context_id: NonZeroU16,
    input_style: InputStyle,
    preedit_spot: Point,
    locale: BString,
    pub user_data: T,
}

impl<T> InputContext<T> {
    pub fn new(
        client_win: u32,
        input_method_id: NonZeroU16,
        input_context_id: NonZeroU16,
        locale: BString,
        user_data: T,
    ) -> Self {
        Self {
            client_win,
            app_win: None,
            app_focus_win: None,
            input_method_id,
            input_context_id,
            input_style: InputStyle::empty(),
            preedit_spot: Point { x: 0, y: 0 },
            locale,
            user_data,
        }
    }

    pub fn client_win(&self) -> u32 {
        self.client_win
    }

    pub fn app_win(&self) -> Option<NonZeroU32> {
        self.app_win
    }

    pub fn app_focus_win(&self) -> Option<NonZeroU32> {
        self.app_focus_win
    }

    pub fn preedit_spot(&self) -> Point {
        self.preedit_spot.clone()
    }

    pub fn input_method_id(&self) -> NonZeroU16 {
        self.input_method_id
    }

    pub fn input_context_id(&self) -> NonZeroU16 {
        self.input_context_id
    }

    pub fn input_style(&self) -> InputStyle {
        self.input_style
    }

    pub fn locale(&self) -> &BStr {
        self.locale.as_ref()
    }
}

fn set_ic_attrs<T>(ic: &mut InputContext<T>, attributes: Vec<Attribute>) {
    for attr in ic_attributes {
        match attr.id {
            IC_INPUTSTYLE => {
                if let Some(style) = xim_parser::read(&attr.value).ok() {
                    ic.input_style = style;
                }
            }
            IC_CLIENTWIN => {
                ic.app_win = xim_parser::read(&attr.value).ok().and_then(NonZeroU32::new);
            }
            IC_FOCUSWIN => {
                ic.app_focus_win = xim_parser::read(&attr.value).ok().and_then(NonZeroU32::new);
            }
            IC_PREEDITATTRS => {
                let mut b = &attr.value[..];
                loop {
                    match xim_parser::read::<Attribute>(b) {
                        Ok(attr) => {
                            b = &b[attr.size()..];
                            match attr.id {
                                IC_SPOTLOCATION => {
                                    if let Ok(spot) = xim_parser::read::<Point>(b) {
                                        ic.preedit_spot = spot;
                                    }
                                }
                                _ => {}
                            }
                        }
                        Err(_) => {
                            break;
                        }
                    }
                }
            }
            _ => {}
        }
    }
}

pub struct InputMethod<T> {
    pub(crate) locale: BString,
    pub(crate) input_contexts: ImVec<InputContext<T>>,
}

impl<T> InputMethod<T> {
    pub fn new(locale: BString) -> Self {
        Self {
            locale,
            input_contexts: ImVec::new(),
        }
    }

    pub fn clone_locale(&self) -> BString {
        self.locale.clone()
    }

    pub fn new_ic(&mut self, ic: InputContext<T>) -> (NonZeroU16, &mut InputContext<T>) {
        self.input_contexts.new_item(ic)
    }

    pub fn remove_input_context(&mut self, ic_id: u16) -> Result<InputContext<T>, ServerError> {
        self.input_contexts
            .remove_item(ic_id)
            .ok_or(ServerError::ClientNotExists)
    }

    pub fn get_input_context(&mut self, ic_id: u16) -> Result<&mut InputContext<T>, ServerError> {
        self.input_contexts
            .get_item(ic_id)
            .ok_or(ServerError::ClientNotExists)
    }
}

const IC_INPUTSTYLE: u16 = 0;
const IC_CLIENTWIN: u16 = 1;
const IC_FOCUSWIN: u16 = 2;
const IC_PREEDITATTRS: u16 = 3;
const IC_SPOTLOCATION: u16 = 4;
const IC_NESTED_SEP: u16 = 30;

pub struct XimConnection<T> {
    pub(crate) client_win: u32,
    pub(crate) disconnected: bool,
    pub(crate) last_focused: Option<(NonZeroU16, NonZeroU16)>,
    pub(crate) input_methods: ImVec<InputMethod<T>>,
}

impl<T> XimConnection<T> {
    pub fn new(client_win: u32) -> Self {
        Self {
            client_win,
            disconnected: false,
            last_focused: None,
            input_methods: ImVec::new(),
        }
    }

    pub fn disconnect<S: ServerCore + Server, H: ServerHandler<S, InputContextData = T>>(
        &mut self,
        handler: &mut H,
    ) {
        for (_id, im) in self.input_methods.drain() {
            for (_id, ic) in im.input_contexts {
                handler.handle_destory_ic(ic);
            }
        }

        self.disconnected = true;
    }

    fn get_input_method(&mut self, id: u16) -> Result<&mut InputMethod<T>, ServerError> {
        self.input_methods
            .get_item(id)
            .ok_or(ServerError::ClientNotExists)
    }

    fn remove_input_method(&mut self, id: u16) -> Result<InputMethod<T>, ServerError> {
        self.input_methods
            .remove_item(id)
            .ok_or(ServerError::ClientNotExists)
    }

    pub(crate) fn handle_request<
        S: ServerCore + Server,
        H: ServerHandler<S, InputContextData = T>,
    >(
        &mut self,
        server: &mut S,
        req: Request,
        handler: &mut H,
    ) -> Result<(), ServerError> {
        match req {
            Request::Error {
                code,
                detail,
                flag: _,
                input_method_id: _,
                input_context_id: _,
            } => {
                // TODO: handle error

                log::error!("XIM ERROR! code: {:?}, detail: {}", code, detail);
            }

            Request::Connect { .. } => {
                server.send_req(
                    self.client_win,
                    Request::ConnectReply {
                        server_major_protocol_version: 1,
                        server_minor_protocol_version: 0,
                    },
                )?;
                handler.handle_connect(server)?;
            }

            Request::Disconnect {} => {
                self.disconnect(handler);
                server.send_req(self.client_win, Request::DisconnectReply {})?;
            }

            Request::Open { locale } => {
                let (input_method_id, _im) = self.input_methods.new_item(InputMethod::new(locale));

                server.send_req(
                    self.client_win,
                    Request::OpenReply {
                        input_method_id: input_method_id.get(),
                        im_attrs: vec![Attr {
                            id: 0,
                            name: AttributeName::QueryInputStyle,
                            ty: AttrType::Style,
                        }],
                        ic_attrs: vec![
                            Attr {
                                id: IC_INPUTSTYLE,
                                name: AttributeName::InputStyle,
                                ty: AttrType::Long,
                            },
                            Attr {
                                id: IC_CLIENTWIN,
                                name: AttributeName::ClientWindow,
                                ty: AttrType::Window,
                            },
                            Attr {
                                id: IC_FOCUSWIN,
                                name: AttributeName::FocusWindow,
                                ty: AttrType::Window,
                            },
                            Attr {
                                id: IC_PREEDITATTRS,
                                name: AttributeName::PreeditAttributes,
                                ty: AttrType::NestedList,
                            },
                            Attr {
                                id: IC_SPOTLOCATION,
                                name: AttributeName::SpotLocation,
                                ty: AttrType::XPoint,
                            },
                            Attr {
                                id: IC_NESTED_SEP,
                                name: AttributeName::SeparatorofNestedList,
                                ty: AttrType::Separator,
                            },
                        ],
                    },
                )?;
            }

            Request::CreateIc {
                input_method_id,
                ic_attributes,
            } => {
                let client_win = self.client_win;
                let im = self.get_input_method(input_method_id)?;
                let mut ic = InputContext::new(
                    client_win,
                    NonZeroU16::new(input_method_id).unwrap(),
                    NonZeroU16::new(1).unwrap(),
                    im.clone_locale(),
                    handler.new_ic_data(),
                );
                set_ic_attrs(&mut ic, ic_attributes);
                let (input_context_id, ic) = im.new_ic(ic);
                ic.input_context_id = input_context_id;

                server.send_req(
                    ic.client_win(),
                    Request::CreateIcReply {
                        input_method_id: input_method_id,
                        input_context_id: input_context_id.get(),
                    },
                )?;

                handler.handle_create_ic(server, ic)?;
            }

            Request::DestoryIc {
                input_context_id,
                input_method_id,
            } => {
                handler.handle_destory_ic(
                    self.get_input_method(input_method_id)?
                        .remove_input_context(input_context_id)?,
                );
                server.send_req(
                    self.client_win,
                    Request::DestroyIcReply {
                        input_method_id,
                        input_context_id,
                    },
                )?;
            }

            Request::Close { input_method_id } => {
                for (_id, ic) in self.remove_input_method(input_method_id)?.input_contexts {
                    handler.handle_destory_ic(ic);
                }

                server.send_req(self.client_win, Request::CloseReply { input_method_id })?;
            }

            Request::QueryExtension {
                input_method_id, ..
            } => {
                // Extension not supported now
                server.send_req(
                    self.client_win,
                    Request::QueryExtensionReply {
                        input_method_id,
                        extensions: Vec::new(),
                    },
                )?;
            }
            Request::EncodingNegotiation {
                input_method_id,
                encodings,
                ..
            } => {
                match encodings
                    .iter()
                    .position(|e| e.starts_with(b"COMPOUND_TEXT"))
                {
                    Some(pos) => {
                        server.send_req(
                            self.client_win,
                            Request::EncodingNegotiationReply {
                                input_method_id,
                                category: 0,
                                index: pos as u16,
                            },
                        )?;
                    }
                    None => {
                        server.send_req(
                            self.client_win,
                            Request::Error {
                                input_method_id,
                                input_context_id: 0,
                                flag: ErrorFlag::INPUTMETHODIDVALID,
                                code: ErrorCode::BadName,
                                detail: "Only COMPOUND_TEXT encoding is supported".into(),
                            },
                        )?;
                    }
                }
            }
            Request::GetImValues {
                input_method_id,
                im_attributes,
            } => {
                let mut out = Vec::with_capacity(im_attributes.len());

                for id in im_attributes {
                    match id {
                        0 => {
                            out.push(Attribute {
                                id,
                                value: xim_parser::write_to_vec(InputStyleList {
                                    styles: handler.input_styles().as_ref().to_vec(),
                                }),
                            });
                        }
                        _ => {
                            return server.error(
                                self.client_win,
                                ErrorCode::BadName,
                                "Unknown im attribute id".into(),
                                NonZeroU16::new(input_method_id),
                                None,
                            );
                        }
                    }
                }

                server.send_req(
                    self.client_win,
                    Request::GetImValuesReply {
                        input_method_id,
                        im_attributes: out,
                    },
                )?;
            }

            Request::SetIcValues {
                input_context_id,
                input_method_id,
                ic_attributes,
            } => {
                let ic = self
                    .get_input_method(input_method_id)?
                    .get_input_context(input_context_id)?;

                set_ic_attrs(ic, ic_attributes);

                server.send_req(
                    ic.client_win,
                    Request::SetIcValuesReply {
                        input_method_id,
                        input_context_id,
                    },
                )?;
            }

            Request::SetIcFocus {
                input_method_id,
                input_context_id,
            } => {
                let ic = self
                    .get_input_method(input_method_id)?
                    .get_input_context(input_context_id)?;
                self.last_focused = Some((ic.input_method_id(), ic.input_context_id()));
            }

            Request::UnsetIcFocus {
                input_method_id,
                input_context_id,
            } => {
                let _ic = self
                    .get_input_method(input_method_id)?
                    .get_input_context(input_context_id)?;
                self.last_focused = None;
            }

            Request::PreeditCaretReply {
                input_method_id,
                input_context_id,
                position,
            } => {
                let ic = self
                    .get_input_method(input_method_id)?
                    .get_input_context(input_context_id)?;

                handler.handle_caret(server, ic, position)?;
            }

            Request::PreeditStartReply {
                input_method_id,
                input_context_id,
                return_value: _,
            } => {
                let ic = self
                    .get_input_method(input_method_id)?
                    .get_input_context(input_context_id)?;

                handler.handle_preedit_start(server, ic)?;
            }

            Request::ForwardEvent {
                input_method_id,
                input_context_id,
                serial_number,
                flag,
                xev,
            } => {
                let ev = server.deserialize_event(&xev);
                let input_context = self
                    .get_input_method(input_method_id)?
                    .get_input_context(input_context_id)?;
                let consumed = handler.handle_forward_event(server, input_context, &ev)?;

                if !consumed {
                    server.send_req(
                        self.client_win,
                        Request::ForwardEvent {
                            input_method_id,
                            input_context_id,
                            serial_number,
                            flag: ForwardEventFlag::empty(),
                            xev,
                        },
                    )?;
                }

                if flag.contains(ForwardEventFlag::SYNCHRONOUS) {
                    server.send_req(
                        self.client_win,
                        Request::SyncReply {
                            input_method_id,
                            input_context_id,
                        },
                    )?;
                }
            }
            _ => {
                log::warn!("Unknown request: {:?}", req);
            }
        }

        Ok(())
    }
}

pub struct XimConnections<T> {
    pub(crate) connections: AHashMap<u32, XimConnection<T>>,
}

impl<T> XimConnections<T> {
    pub fn new() -> Self {
        Self {
            connections: AHashMap::new(),
        }
    }

    pub fn new_connection(&mut self, com_win: u32, client_win: u32) {
        self.connections
            .insert(com_win, XimConnection::new(client_win));
    }

    pub fn get_connection(&mut self, com_win: u32) -> Option<&mut XimConnection<T>> {
        self.connections.get_mut(&com_win)
    }

    pub fn remove_connection(&mut self, com_win: u32) -> Option<XimConnection<T>> {
        self.connections.remove(&com_win)
    }
}
