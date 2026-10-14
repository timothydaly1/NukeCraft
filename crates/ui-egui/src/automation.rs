//! Automation registry: every interactive element registers a stable id and its screen rect each
//! frame (`graph.node.Blur1`, `viewer.fit`, `props.Blur1.size`), so agents and the headless
//! snapshot tool can click, drag and inspect the UI by id.

use egui::{Pos2, Rect};

#[derive(Clone, Debug, PartialEq)]
pub struct Element {
    pub id: String,
    pub label: String,
    pub rect: Rect,
}

#[derive(Default)]
pub struct Registry {
    elements: Vec<Element>,
    /// The previous frame's elements, for lookups between frames.
    previous: Vec<Element>,
}

impl Registry {
    /// Starts a new frame: this frame's elements become the lookup set.
    pub fn begin_frame(&mut self) {
        self.previous = std::mem::take(&mut self.elements);
    }

    pub fn add(&mut self, id: impl Into<String>, rect: Rect, label: impl Into<String>) {
        self.elements.push(Element { id: id.into(), label: label.into(), rect });
    }

    /// An element by id, from the last complete frame (or this one).
    pub fn find(&self, id: &str) -> Option<&Element> {
        self.previous.iter().rev().chain(self.elements.iter().rev()).find(|e| e.id == id)
    }

    /// The centre of an element, for clicks.
    pub fn center(&self, id: &str) -> Option<Pos2> {
        self.find(id).map(|e| e.rect.center())
    }

    /// Elements of the last complete frame whose id starts with `prefix`.
    pub fn query(&self, prefix: &str) -> Vec<&Element> {
        self.previous.iter().filter(|e| e.id.starts_with(prefix)).collect()
    }
}
