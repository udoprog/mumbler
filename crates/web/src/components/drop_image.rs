use core::cell::RefCell;

use std::rc::{Rc, Weak};

use api::{Image, Key, RemoteId, RemoteObject, Role, Type, Value, Vec3};
use gloo::file::callbacks::{FileReader, read_as_bytes};
use musli_web::web03::prelude::*;
use wasm_bindgen::prelude::*;
use web_sys::{File, HtmlImageElement};
use yew::prelude::*;

use crate::error::Error;

use super::{SetupChannel, TemporaryUrl};

#[derive(Debug)]
pub(crate) enum DropImageResult {
    Ok,
    Err(Error),
    RemoteObject(RemoteObject),
    Image(Image),
}

struct Inner {
    inner: Weak<RefCell<Inner>>,
    ty: Type,
    _setup_channel: SetupChannel,
    _create_object: ws::Request,
    _upload_image: ws::Request,
    image: Option<HtmlImageElement>,
    image_onerror: Option<Closure<dyn FnMut()>>,
    image_onload: Option<Closure<dyn FnMut()>>,
    url: Option<TemporaryUrl>,
    bytes: Option<Vec<u8>>,
    content_type: String,
    file_reader: Option<FileReader>,
    pixel_size: Option<(u32, u32)>,
    position: Vec3,
    channel: Option<ws::Channel>,
    onresult: Callback<DropImageResult>,
}

pub(crate) struct DropImage {
    _inner: Rc<RefCell<Inner>>,
}

impl DropImage {
    pub(crate) fn new(
        ws: ws::Handle,
        ty: Type,
        content_type: String,
        file: File,
        position: Vec3,
        onresult: Callback<DropImageResult>,
    ) -> Result<Self, Error> {
        let url = TemporaryUrl::create(&file, onresult.reform(DropImageResult::Err))?;

        let image = HtmlImageElement::new()?;
        let gloo_file = gloo::file::File::from(file);

        Ok(Self {
            _inner: Rc::new_cyclic(|inner: &Weak<RefCell<Inner>>| {
                let setup_channel = SetupChannel::new(
                    ws,
                    Callback::from({
                        let inner = inner.clone();

                        move |result| {
                            let Some(inner) = inner.upgrade() else {
                                return;
                            };

                            let mut inner = inner.borrow_mut();

                            match result {
                                Ok(channel) => {
                                    inner.channel = Some(channel);
                                    inner.try_upload_image();
                                }
                                Err(error) => {
                                    inner.onresult.emit(DropImageResult::Err(error));
                                }
                            }
                        }
                    }),
                );

                let file_reader = read_as_bytes(&gloo_file, {
                    let inner = inner.clone();

                    move |result| {
                        let Some(inner) = inner.upgrade() else {
                            return;
                        };

                        let mut inner = inner.borrow_mut();

                        match result {
                            Ok(data) => {
                                inner.bytes = Some(data);
                                inner.file_reader = None;
                                inner.try_upload_image();
                            }
                            Err(error) => {
                                inner
                                    .onresult
                                    .emit(DropImageResult::Err(Error::from(error)));
                            }
                        }
                    }
                });

                let image_onload = Closure::<dyn FnMut()>::new({
                    let inner = inner.clone();

                    move || {
                        let Some(inner) = inner.upgrade() else {
                            return;
                        };

                        let mut inner = inner.borrow_mut();

                        let Some(image) = inner.image.take() else {
                            return;
                        };

                        let width = image.natural_width();
                        let height = image.natural_height();

                        image.set_onload(None);
                        image.set_onerror(None);
                        image.remove();

                        inner.image_onerror = None;
                        inner.image_onload = None;
                        inner.url = None;
                        inner.pixel_size = Some((width, height));
                        inner.try_upload_image();
                    }
                });

                let image_onerror = Closure::<dyn FnMut()>::new({
                    let inner = inner.clone();

                    move || {
                        let Some(inner) = inner.upgrade() else {
                            return;
                        };

                        let mut inner = inner.borrow_mut();

                        let Some(image) = inner.image.take() else {
                            return;
                        };

                        image.set_onload(None);
                        image.set_onerror(None);
                        image.remove();

                        inner.image_onerror = None;
                        inner.image_onload = None;
                        inner.url = None;

                        inner.onresult.emit(DropImageResult::Err(Error::message(
                            "Error loading dropped image",
                        )));
                    }
                });

                image.set_onload(Some(image_onload.as_ref().unchecked_ref()));
                image.set_onerror(Some(image_onerror.as_ref().unchecked_ref()));
                image.set_src(&url);

                RefCell::new(Inner {
                    inner: inner.clone(),
                    ty,
                    _setup_channel: setup_channel,
                    _create_object: ws::Request::new(),
                    _upload_image: ws::Request::new(),
                    image: Some(image),
                    image_onerror: Some(image_onerror),
                    image_onload: Some(image_onload),
                    url: Some(url),
                    bytes: None,
                    content_type,
                    file_reader: Some(file_reader),
                    pixel_size: None,
                    position,
                    channel: None,
                    onresult,
                })
            }),
        })
    }
}

impl Inner {
    fn create_object(&mut self, id: RemoteId) {
        let Some(channel) = &self.channel else {
            return;
        };

        let Some((width, height)) = self.pixel_size.map(world_size) else {
            return;
        };

        let position = self.position;

        let transform = api::Transform::new(position, api::Vec3::FORWARD);

        let props = api::Properties::from_iter([
            (Key::HIDDEN, Value::from(true)),
            (Key::IMAGE_ID, Value::from(id.id)),
            (Key::TRANSFORM, Value::from(transform)),
            (Key::WIDTH, Value::from(width)),
            (Key::HEIGHT, Value::from(height)),
        ]);

        let body = api::CreateObjectRequest {
            ty: self.ty,
            props: props.clone(),
        };

        let inner = self.inner.clone();

        let callback = Callback::from(
            move |result: Result<ws::Packet<api::CreateObject>, ws::Error>| {
                let Some(inner) = inner.upgrade() else {
                    return;
                };

                let inner = inner.borrow_mut();

                let packet = match result {
                    Ok(packet) => packet,
                    Err(error) => {
                        inner
                            .onresult
                            .emit(DropImageResult::Err(Error::from(error)));
                        return;
                    }
                };

                match packet.decode() {
                    Ok(body) => {
                        inner
                            .onresult
                            .emit(DropImageResult::RemoteObject(body.object));
                        inner.onresult.emit(DropImageResult::Ok);
                    }
                    Err(error) => {
                        inner
                            .onresult
                            .emit(DropImageResult::Err(Error::from(error)));
                    }
                };
            },
        );

        self._create_object = channel.request().body(body).on_packet(callback).send();
    }

    fn try_upload_image(&mut self) {
        if !self.is_ready_for_upload() {
            return;
        }

        let Some(channel) = &self.channel else {
            return;
        };

        let Some((width, height)) = self.pixel_size else {
            return;
        };

        let Some(data) = self.bytes.take() else {
            return;
        };

        let inner = self.inner.clone();

        let callback = Callback::from(
            move |result: Result<ws::Packet<api::UploadImage>, ws::Error>| {
                let Some(inner) = inner.upgrade() else {
                    return;
                };

                let mut inner = inner.borrow_mut();

                let packet = match result {
                    Ok(packet) => packet,
                    Err(error) => {
                        inner
                            .onresult
                            .emit(DropImageResult::Err(Error::from(error)));
                        return;
                    }
                };

                match packet.decode() {
                    Ok(body) => {
                        let id = body.image.id;
                        inner.onresult.emit(DropImageResult::Image(body.image));
                        inner.create_object(id);
                    }
                    Err(error) => {
                        inner
                            .onresult
                            .emit(DropImageResult::Err(Error::from(error)));
                    }
                }
            },
        );

        let body = api::UploadImageRequestRef {
            content_type: &self.content_type,
            role: Role::STATIC,
            crop: api::CropRegion {
                x1: 0,
                y1: 0,
                x2: width,
                y2: height,
            },
            sizing: api::ImageSizing::Crop,
            size: 512,
            data: &data,
        };

        self._upload_image = channel.request().body(body).on_packet(callback).send();
    }

    #[inline]
    fn is_ready_for_upload(&self) -> bool {
        self.channel.is_some() && self.pixel_size.is_some() && self.bytes.is_some()
    }
}

#[inline]
fn world_size((width, height): (u32, u32)) -> (f32, f32) {
    if width == 0 || height == 0 {
        return (2.0, 2.0);
    }

    let width = width as f32;
    let height = height as f32;

    if width >= height {
        (2.0, 2.0 * (height / width))
    } else {
        (2.0 * (width / height), 2.0)
    }
}
