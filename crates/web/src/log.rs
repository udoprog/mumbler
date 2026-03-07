use core::fmt;

use std::cell::RefCell;
use std::collections::VecDeque;
use std::rc::{Rc, Weak};

use slab::Slab;
use web_sys::js_sys::Date;
use yew::Callback;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Severity {
    Info,
    Error,
}

impl Severity {
    pub fn as_str(&self) -> &'static str {
        match self {
            Severity::Info => "info",
            Severity::Error => "error",
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct ErrorEntry {
    pub timestamp: f64,
    pub component: String,
    pub error: String,
    pub severity: Severity,
}

impl ErrorEntry {
    pub fn formatted_time(&self) -> String {
        let date = Date::new(&self.timestamp.into());
        let hours = date.get_hours();
        let minutes = date.get_minutes();
        let seconds = date.get_seconds();
        let millis = date.get_milliseconds();
        format!("{:02}:{:02}:{:02}.{:03}", hours, minutes, seconds, millis)
    }
}

#[derive(Debug, Clone)]
pub struct Log {
    inner: Rc<RefCell<ErrorLogInner>>,
}

impl PartialEq for Log {
    fn eq(&self, other: &Self) -> bool {
        Rc::ptr_eq(&self.inner, &other.inner)
    }
}

#[derive(Debug)]
struct ErrorLogInner {
    entries: VecDeque<ErrorEntry>,
    max_entries: usize,
    listeners: Slab<Callback<usize>>,
}

pub struct ListenerHandle {
    id: usize,
    inner: Weak<RefCell<ErrorLogInner>>,
}

impl Drop for ListenerHandle {
    fn drop(&mut self) {
        if let Some(inner) = self.inner.upgrade() {
            inner.borrow_mut().listeners.remove(self.id);
        }
    }
}

impl Log {
    pub fn new() -> Self {
        Self {
            inner: Rc::new(RefCell::new(ErrorLogInner {
                entries: VecDeque::new(),
                max_entries: 100,
                listeners: Slab::new(),
            })),
        }
    }

    pub fn add_listener(&self, callback: Callback<usize>) -> ListenerHandle {
        let mut inner = self.inner.borrow_mut();
        let id = inner.listeners.insert(callback);

        ListenerHandle {
            id,
            inner: Rc::downgrade(&self.inner),
        }
    }

    fn notify_listeners(&self) {
        let inner = self.inner.borrow();
        let len = inner.entries.len();

        for listener in inner.listeners.iter() {
            listener.1.emit(len);
        }
    }

    pub fn log(&self, component: impl fmt::Display, error: impl fmt::Display, severity: Severity) {
        let timestamp = Self::now();
        let entry = ErrorEntry {
            timestamp,
            component: component.to_string(),
            error: error.to_string(),
            severity,
        };

        let mut inner = self.inner.borrow_mut();
        inner.entries.push_back(entry);

        // Keep only the most recent entries
        while inner.entries.len() > inner.max_entries {
            inner.entries.pop_front();
        }
        drop(inner);

        self.notify_listeners();
    }

    #[allow(unused)]
    pub fn log_info(&self, component: impl fmt::Display, message: impl fmt::Display) {
        self.log(component, message, Severity::Info);
    }

    pub fn error(&self, component: impl fmt::Display, error: impl fmt::Display) {
        self.log(component, error, Severity::Error);
    }

    pub fn entries(&self) -> Vec<ErrorEntry> {
        self.inner.borrow().entries.iter().cloned().collect()
    }

    pub fn clear(&self) {
        let mut inner = self.inner.borrow_mut();
        inner.entries.clear();
        drop(inner);

        self.notify_listeners();
    }

    fn now() -> f64 {
        Date::now()
    }
}

impl Default for Log {
    fn default() -> Self {
        Self::new()
    }
}
