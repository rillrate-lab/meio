use crate::server::Extractor;
use hyper::{Body, Request};

pub struct PlainExtractor {
    path: String,
}

impl PlainExtractor {
    pub fn new(path: String) -> Self {
        Self { path }
    }
}

pub struct PlainPath {
    pub path: String,
}

impl Extractor for PlainExtractor {
    type Request = PlainPath;

    fn try_extract(&self, request: &Request<Body>) -> Option<Self::Request> {
        if request.uri().path() == self.path {
            let req = PlainPath {
                path: self.path.clone(),
            };
            Some(req)
        } else {
            None
        }
    }
}
