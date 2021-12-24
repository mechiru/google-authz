use std::{convert::TryFrom as _, fmt};

use hyper::{http::uri::PathAndQuery, Request, Uri};

#[derive(Clone)]
pub(super) struct ApiKey {
    value: String,
}

impl ApiKey {
    pub fn new(key: impl Into<String>) -> Self {
        Self { value: key.into() }
    }

    #[inline]
    pub fn add_query<B>(&self, req: Request<B>) -> Request<B> {
        let (mut head, body) = req.into_parts();
        let s = {
            let mut s = head.uri.path().to_owned();
            s.push('?');
            if let Some(q) = head.uri.query() {
                s.push_str(q);
                if !q.ends_with('&') {
                    s.push('&')
                }
            }
            s.push_str("key=");
            s.push_str(&self.value);
            s
        };

        let mut parts = head.uri.into_parts();
        parts.path_and_query = Some(PathAndQuery::try_from(s).unwrap());

        head.uri = Uri::from_parts(parts).unwrap();
        Request::from_parts(head, body)
    }
}

impl fmt::Debug for ApiKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ApiKey").finish()
    }
}
