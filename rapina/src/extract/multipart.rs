use crate::error::Error;
use crate::extract::{FromRequest, PathParams};
use crate::state::AppState;
use futures_util::stream::StreamExt;
use http::header::CONTENT_TYPE;
use hyper::body::Incoming;
use std::io;
use std::sync::Arc;

/// Extractor for multipart form data.
///
/// This extractor provides access to the individual fields of a multipart request.
/// It uses `multer` under the hood for efficient streaming.
///
/// # Examples
///
/// ```rust,ignore
/// use rapina::prelude::*;
///
/// #[post("/upload")]
/// async fn upload(mut multipart: Multipart) -> Result<String> {
///     while let Some(mut field) = multipart.next_field().await? {
///         let name = field.name().unwrap_or("unknown").to_string();
///         let file_name = field.file_name().map(|s| s.to_string());
///
///         if let Some(file_name) = file_name {
///             println!("Uploading file: {} as field: {}", file_name, name);
///             let data = field.bytes().await?;
///             // Process file data...
///         } else {
///             let text = field.text().await?;
///             println!("Field: {} = {}", name, text);
///         }
///     }
///     Ok("Upload successful".to_string())
/// }
/// ```
pub struct Multipart {
    inner: multer::Multipart<'static>,
}

impl FromRequest for Multipart {
    async fn from_request(
        req: http::Request<Incoming>,
        _params: &PathParams,
        _state: &Arc<AppState>,
    ) -> Result<Self, Error> {
        let boundary = req
            .headers()
            .get(CONTENT_TYPE)
            .and_then(|v| v.to_str().ok())
            .and_then(|v| multer::parse_boundary(v).ok())
            .ok_or_else(|| Error::bad_request("invalid or missing multipart boundary"))?;

        let stream =
            http_body_util::BodyStream::new(req.into_body()).filter_map(|result| async move {
                match result {
                    Ok(frame) => frame.into_data().ok().map(Ok::<_, multer::Error>),
                    Err(e) => Some(Err(multer::Error::StreamReadFailed(Box::new(
                        io::Error::other(e),
                    )))),
                }
            });

        Ok(Self::new_with_stream(stream, boundary))
    }
}

impl Multipart {
    /// Creates a new `Multipart` instance from a stream and boundary.
    pub(crate) fn new_with_stream<S>(stream: S, boundary: impl Into<String>) -> Self
    where
        S: futures_util::Stream<Item = Result<bytes::Bytes, multer::Error>> + Send + 'static,
    {
        let multipart = multer::Multipart::new(stream, boundary);
        Multipart { inner: multipart }
    }

    /// Yields the next field from the multipart body.
    ///
    /// Returns `Ok(Some(field))` if a field is available, `Ok(None)` if the end of
    /// the stream is reached, or an error if the request is malformed.
    pub async fn next_field(&mut self) -> Result<Option<Field<'static>>, Error> {
        match self.inner.next_field().await {
            Ok(Some(inner)) => Ok(Some(Field { inner })),
            Ok(None) => Ok(None),
            Err(e) => Err(Error::bad_request(format!("multipart error: {}", e))),
        }
    }
}

/// A single field in a multipart body.
///
/// Provides methods to access field metadata and stream its contents.
pub struct Field<'a> {
    inner: multer::Field<'a>,
}

impl<'a> Field<'a> {
    /// Returns the name of the field from the `Content-Disposition` header.
    pub fn name(&self) -> Option<&str> {
        self.inner.name()
    }

    /// Returns the filename of the field from the `Content-Disposition` header.
    pub fn file_name(&self) -> Option<&str> {
        self.inner.file_name()
    }

    /// Returns the content type of the field from the `Content-Type` header.
    pub fn content_type(&self) -> Option<&str> {
        self.inner.content_type().map(|c| c.as_ref())
    }

    /// Reads the next chunk of bytes from the field.
    ///
    /// Useful for streaming large files without loading the entire content into memory.
    pub async fn chunk(&mut self) -> Result<Option<bytes::Bytes>, Error> {
        self.inner
            .chunk()
            .await
            .map_err(|e| Error::bad_request(format!("multipart field error: {}", e)))
    }

    /// Collects the remaining bytes from the field into a `Bytes`.
    pub async fn bytes(self) -> Result<bytes::Bytes, Error> {
        self.inner
            .bytes()
            .await
            .map_err(|e| Error::bad_request(format!("multipart field error: {}", e)))
    }

    /// Collects the remaining bytes from the field into a `String`.
    pub async fn text(self) -> Result<String, Error> {
        self.inner
            .text()
            .await
            .map_err(|e| Error::bad_request(format!("multipart field error: {}", e)))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use bytes::Bytes;
    use futures_util::stream;

    #[tokio::test]
    async fn test_multipart_extraction() {
        let boundary = "boundary";
        let body = format!(
            "--{boundary}\r\n\
             Content-Disposition: form-data; name=\"foo\"\r\n\
             \r\n\
             bar\r\n\
             --{boundary}--\r\n"
        );

        let stream = stream::once(async move { Ok::<_, multer::Error>(Bytes::from(body)) });
        let mut multipart = Multipart::new_with_stream(stream, boundary);

        let field = multipart.next_field().await.unwrap().unwrap();
        assert_eq!(field.name(), Some("foo"));
        assert_eq!(field.text().await.unwrap(), "bar");

        assert!(multipart.next_field().await.unwrap().is_none());
    }

    #[tokio::test]
    async fn test_multipart_multiple_fields() {
        let boundary = "boundary";
        let body = format!(
            "--{boundary}\r\n\
             Content-Disposition: form-data; name=\"foo\"\r\n\
             \r\n\
             bar\r\n\
             --{boundary}\r\n\
             Content-Disposition: form-data; name=\"baz\"; filename=\"test.txt\"\r\n\
             Content-Type: text/plain\r\n\
             \r\n\
             qux\r\n\
             --{boundary}--\r\n"
        );

        let stream = stream::once(async move { Ok::<_, multer::Error>(Bytes::from(body)) });
        let mut multipart = Multipart::new_with_stream(stream, boundary);

        let field1 = multipart.next_field().await.unwrap().unwrap();
        assert_eq!(field1.name(), Some("foo"));
        assert_eq!(field1.text().await.unwrap(), "bar");

        let field2 = multipart.next_field().await.unwrap().unwrap();
        assert_eq!(field2.name(), Some("baz"));
        assert_eq!(field2.file_name(), Some("test.txt"));
        assert_eq!(field2.content_type(), Some("text/plain"));
        assert_eq!(field2.text().await.unwrap(), "qux");

        assert!(multipart.next_field().await.unwrap().is_none());
    }
}
