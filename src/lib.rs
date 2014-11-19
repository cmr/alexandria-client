//! API client library for Alexandria.

#![feature(macro_rules)]

extern crate alexandria;
extern crate hyper;
extern crate serialize;
extern crate url;

use serialize::json;
use hyper::{status, Url};
use hyper::client::Request;

macro_rules! try_error {
    ($e:expr, $kind:expr) => (
        match $e {
            Ok(res) => res,
            Err(e) => return Err($kind(e)),
        }
    )
}

pub struct Unauth;
pub struct Auth;

/// An Alexandria server.
///
/// An Alexandria server can be queried for contained books, and books can be checked in/checked
/// out after authentication.
pub struct Server<T> {
    base_url: String,
    proto: T,
}

/// Error type for API calls.
///
/// In general, failure can come from either the HTTP request, JSON decoding, or the Alexandria
/// server itself.
#[deriving(Show)]
pub enum Error {
    HttpError(hyper::HttpError),
    JsonError(json::DecoderError),
    IoError(std::io::IoError),
    NotFound,
    InternalError,
    /// The server didn't return HTTP 200, something went wrong.
    ApiSaidNo,
    /// Couldn't authenticate with the given credentials.
    AuthError,
    /// For some reason, it returned HTTP 200 but also null.
    GotNull,
}

pub type APIResult<T> = Result<T, Error>;

fn do_req<'a, T, U>(url: Url, data: Option<T>,
                    cb: fn(Url) -> hyper::HttpResult<Request<hyper::net::Fresh>>)
                    -> APIResult<U>
        where
        T : serialize::Encodable<json::Encoder<'a>, std::io::IoError>,
        U : serialize::Decodable<json::Decoder, json::DecoderError> {
    let req = try_error!(cb(url), Error::HttpError);
    let mut req = try_error!(req.start(), Error::HttpError);
    match data {
        Some(obj) => {
            let enc = json::encode(&obj);
            try_error!(req.write(enc.as_bytes()), Error::IoError);
        },
        None => ()
    }
    let mut resp = try_error!(req.send(), Error::HttpError);
    match resp.status {
        status::NotFound => return Err(Error::NotFound),
        status::InternalServerError => return Err(Error::InternalError),
        status::Unauthorized => return Err(Error::AuthError),
        status::Ok => (),
        _ => return Err(Error::ApiSaidNo),
    }
    let str = try_error!(resp.read_to_string(), Error::IoError);
    let val: U = try_error!(json::decode(str.as_slice()), Error::JsonError);
    Ok(val)
}

fn do_get<T: serialize::Decodable<json::Decoder, json::DecoderError>>(url: Url) -> APIResult<T> {
    do_req(url, None::<i32>, Request::get)
}


fn do_post<'a, T, U>(url: Url, body: Option<U>) -> APIResult<T> where
        T : serialize::Decodable<json::Decoder, json::DecoderError>,
        U : serialize::Encodable<json::Encoder<'a>, std::io::IoError> {
    do_req(url, body, Request::post)
}

fn do_put<'a, T, U>(url: Url, body: Option<U>) -> APIResult<T> where
        T : serialize::Decodable<json::Decoder, json::DecoderError>,
        U : serialize::Encodable<json::Encoder<'a>, std::io::IoError> {
    do_req(url, body, Request::put)
}

fn do_delete<'a, T, U>(url: Url, body: Option<U>) -> APIResult<T> where
        T : serialize::Decodable<json::Decoder, json::DecoderError>,
        U : serialize::Encodable<json::Encoder<'a>, std::io::IoError> {
    do_req(url, body, Request::delete)
}


trait Proto {
    fn proto(&self) -> &'static str;
}

impl Proto for Auth {
    fn proto(&self) -> &'static str { "http" }
}

impl Proto for Unauth {
    fn proto(&self) -> &'static str { "http" }
}

impl Server<Unauth> {
    /// Create a new Alexandria server, located at the domain name `base`. When making HTTP
    /// requests, the path is appended to the `base`, with the appropriate protocol.
    ///
    /// Example:
    /// ```rust,norun
    /// # use alexandria_client::Server;
    /// let server = Server::new("http://alexandria.cslabs.clarkson.edu".to_string());
    /// ```
    pub fn new(base: String) -> Result<Server<Unauth>, url::ParseError> {
        match Url::parse(base.as_slice()) {
            Ok(_) => Ok(Server { base_url: base, proto: Unauth }),
            Err(e) => Err(e)
        }
    }
}

// Generic methods should not be able to access privileged data.
impl<T: Proto> Server<T> {
    /// Query for the first `count` books.
    pub fn get_books(&self, count: u32) -> APIResult<Vec<alexandria::Book>> {
        let base = self.base_url.as_slice();
        let url = Url::parse(format!("{}://{}/book?count={}", self.proto.proto(),
                                                              base, count).as_slice());
        do_get(url.ok().unwrap())
    }

    /// Query for a specific book with a specific ISBN..
    pub fn get_book_by_isbn(&self, isbn: &str) -> APIResult<Option<alexandria::Book>> {
        let (base, isbn) = (self.base_url.as_slice(), isbn.as_slice());

        do_get(Url::parse(format!("{}://{}/book/{}", self.proto.proto(), base, isbn)
                          .as_slice()).ok().expect("Invalid ISBN!"))
    }
}

impl Server<Unauth> {
    /// Authenticate to the Alexandria server with a username/password pair.
    ///
    /// After this point, all requests will be made over SSL.
    pub fn authenticate(self, user: &str, pass: &str) -> APIResult<Server<Auth>> {
        let serv = Server {
            base_url: self.base_url,
            proto: Auth,
        };

        let res = do_get(Url::parse(format!("{}://{}/auth?user={}&pass={}", serv.proto.proto(),
                                            serv.base_url.as_slice(), user, pass).as_slice())
                         .ok().expect("TODO: URLEncoding (not your fault)"));

        match res {
            Ok(()) => return Ok(serv),
            Err(e) => return Err(e)
        }
    }
}

impl Server<Auth> {
    /// Checkout the book corresponding to `isbn` to `student_id`, if possible.
    ///
    /// Returns `true` if the checkout was successful, `false` otherwise.
    pub fn checkout(&self, isbn: &str, student_id: &str) -> APIResult<bool> {
        let base = self.base_url.as_slice();
        // todo: deal with this allocation
        let ac = alexandria::ActionRequest {
            action: alexandria::CheckOut,
            isbn: isbn.to_string(),
            student_id: student_id.to_string()
        };
        let url = Url::parse(format!("{}://{}/checkout", self.proto.proto(), base).as_slice())
            .ok().expect("Invalid ISBN or Student ID!");

        do_post(url, Some(ac))
    }

    /// Checkout the book corresponding to `isbn` to `student_id`, if possible.
    ///
    /// Returns `true` if the checkin was successful, `false` otherwise.
    pub fn checkin(&self, isbn: &str, student_id: &str) -> APIResult<bool> {
        let base = self.base_url.as_slice();
        let ac = alexandria::ActionRequest {
            action: alexandria::CheckIn,
            isbn: isbn.to_string(),
            student_id: student_id.to_string()
        };
        let url = Url::parse(format!("{}://{}/checkin", self.proto.proto(), base).as_slice())
            .ok().expect("Invalid ISBN or Student ID!");

        do_post(url, Some(ac))
    }

    /// Update the book with `isbn` to match `book`.
    ///
    /// Returns `true` if the update was successful, `false` otherwise.
    pub fn update_book(&self, isbn: &str, book: &alexandria::Book) -> APIResult<bool> {
        let base = self.base_url.as_slice();
        let url = Url::parse(format!("{}://{}/book/{}", self.proto.proto(), base, isbn).as_slice())
            .ok().expect("Invalid ISBN!");
        do_post(url, Some(book))
    }

    /// Add a book to the library.
    ///
    /// Returns `true` if the add was successful, `false` if a book with that ISBN already exists.
    pub fn add_book(&self, book: &alexandria::Book) -> APIResult<bool> {
        let base = self.base_url.as_slice();
        let url = Url::parse(format!("{}://{}/book", self.proto.proto(), base).as_slice())
            .ok().expect("Invalid ISBN!");
        do_put(url, Some(book))
    }

    /// Remove a book from the library.
    ///
    /// Returns `true` if the deletion was successful, `false` if there are no copies of that book
    /// in the library.
    pub fn delete_book(&self, isbn: &str) -> APIResult<bool> {
        let base = self.base_url.as_slice();
        let url = Url::parse(format!("{}://{}/book/{}", self.proto.proto(), base, isbn).as_slice())
            .ok().expect("Invalid ISBN!");
        do_delete(url, None::<int>)
    }

    /// Register a book in the library.
    pub fn register_book(&self, isbn: &str) -> APIResult<bool> {
        let base = self.base_url.as_slice();
        let url = Url::parse(format!("{}://{}/book/{}", self.proto.proto(), base, isbn).as_slice())
            .ok().expect("Invalid ISBN!");
        do_put(url, Some(isbn))
    }
}
