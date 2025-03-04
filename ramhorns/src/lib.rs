// Ramhorns  Copyright (C) 2019  Maciej Hirsz
//
// This file is part of Ramhorns. This program comes with ABSOLUTELY NO WARRANTY;
// This is free software, and you are welcome to redistribute it under the
// conditions of the GNU General Public License version 3.0.
//
// You should have received a copy of the GNU General Public License
// along with Ramhorns.  If not, see <http://www.gnu.org/licenses/>

//! <img src="https://raw.githubusercontent.com/maciejhirsz/ramhorns/master/ramhorns.svg?sanitize=true" alt="Ramhorns logo" width="250" align="right" style="background: #fff; margin: 0 0 1em 1em;">
//!
//! # Ramhorns
//!
//! Fast [**Mustache**](https://mustache.github.io/) template engine implementation
//! in pure Rust.
//!
//! **Ramhorns** loads and processes templates **at runtime**. It comes with a derive macro
//! which allows for templates to be rendered from native Rust data structures without doing
//! temporary allocations, intermediate `HashMap`s or what have you.
//!
//! With a touch of magic 🎩, the power of friendship 🥂, and a sparkle of
//! [FNV hashing](https://en.wikipedia.org/wiki/Fowler%E2%80%93Noll%E2%80%93Vo_hash_function)
//! ✨, render times easily compete with static template engines like
//! [**Askama**](https://github.com/djc/askama).
//!
//! What else do you want, a sticker?
//!
//! ## Example
//!
//! ```rust
//! use ramhorns::{Template, Content};
//!
//! #[derive(Content)]
//! struct Post<'a> {
//!     title: &'a str,
//!     teaser: &'a str,
//! }
//!
//! #[derive(Content)]
//! struct Blog<'a> {
//!     title: String,        // Strings are cool
//!     posts: Vec<Post<'a>>, // &'a [Post<'a>] would work too
//! }
//!
//! // Standard Mustache action here
//! let source = "<h1>{{title}}</h1>\
//!               {{#posts}}<article><h2>{{title}}</h2><p>{{teaser}}</p></article>{{/posts}}\
//!               {{^posts}}<p>No posts yet :(</p>{{/posts}}";
//!
//! let tpl = Template::new(source).unwrap();
//!
//! let rendered = tpl.render(&Blog {
//!     title: "My Awesome Blog!".to_string(),
//!     posts: vec![
//!         Post {
//!             title: "How I tried Ramhorns and found love 💖",
//!             teaser: "This can happen to you too",
//!         },
//!         Post {
//!             title: "Rust is kinda awesome",
//!             teaser: "Yes, even the borrow checker! 🦀",
//!         },
//!     ]
//! });
//!
//! assert_eq!(rendered, "<h1>My Awesome Blog!</h1>\
//!                       <article>\
//!                           <h2>How I tried Ramhorns and found love 💖</h2>\
//!                           <p>This can happen to you too</p>\
//!                       </article>\
//!                       <article>\
//!                           <h2>Rust is kinda awesome</h2>\
//!                           <p>Yes, even the borrow checker! 🦀</p>\
//!                       </article>");
//! ```

#![warn(missing_docs)]

use std::path::{Path, PathBuf};

use ahash::AHashMap as HashMap;
use beef::Cow;
use std::io::ErrorKind;

mod content;
mod error;
mod template;
pub mod traits;

pub mod encoding;

pub use content::Content;
pub use error::Error;
pub use template::{Section, Template};

#[cfg(feature = "export_derive")]
pub use ramhorns_derive::Content;

/// Aggregator for [`Template`s](./struct.Template.html), that allows them to
/// be loaded from the file system and use partials: `{{>partial}}`
pub struct Ramhorns {
    partials: HashMap<Cow<'static, str>, Template<'static>>,
    dir: PathBuf,
}

impl Ramhorns {
    /// Loads all the `.html` files as templates from the given folder, making them
    /// accessible via their path, joining partials as required. If a custom
    /// extension is wanted, see [from_folder_with_extension]
    /// ```no_run
    /// # use ramhorns::Ramhorns;
    /// let tpls = Ramhorns::from_folder("./templates").unwrap();
    /// let content = "I am the content";
    /// let rendered = tpls.get("hello.html").unwrap().render(&content);
    /// ```
    pub fn from_folder<P: AsRef<Path>>(dir: P) -> Result<Self, Error> {
        Self::from_folder_with_extension(dir, "html")
    }

    /// Loads all files with the extension given in the `extension` parameter as templates
    /// from the given folder, making them accessible via their path, joining partials as
    /// required.
    /// ```no_run
    /// # use ramhorns::Ramhorns;
    /// let tpls = Ramhorns::from_folder_with_extension("./templates", "mustache").unwrap();
    /// let content = "I am the content";
    /// let rendered = tpls.get("hello.mustache").unwrap().render(&content);
    /// ```
    #[inline]
    pub fn from_folder_with_extension<P: AsRef<Path>>(
        dir: P,
        extension: &str,
    ) -> Result<Self, Error> {
        let mut templates = Ramhorns::lazy(dir)?;
        templates.load_folder(&templates.dir.clone(), extension)?;

        Ok(templates)
    }

    /// Extends the template collection with files with `.html` extension
    /// from the given folder, making them accessible via their path, joining partials as
    /// required.
    /// If there is a file with the same name as a  previously loaded template or partial,
    /// it will not be loaded.
    pub fn extend_from_folder<P: AsRef<Path>>(&mut self, dir: P) -> Result<(), Error> {
        self.extend_from_folder_with_extension(dir, "html")
    }

    /// Extends the template collection with files with `extension`
    /// from the given folder, making them accessible via their path, joining partials as
    /// required.
    /// If there is a file with the same name as a  previously loaded template or partial,
    /// it will not be loaded.
    #[inline]
    pub fn extend_from_folder_with_extension<P: AsRef<Path>>(
        &mut self,
        dir: P,
        extension: &str,
    ) -> Result<(), Error> {
        let dir = std::mem::replace(&mut self.dir, dir.as_ref().canonicalize()?);
        self.load_folder(&self.dir.clone(), extension)?;
        self.dir = dir;

        Ok(())
    }

    fn load_folder(&mut self, dir: &Path, extension: &str) -> Result<(), Error> {
        for entry in std::fs::read_dir(dir)? {
            let path = entry?.path();
            if path.is_dir() {
                self.load_folder(&path, extension)?;
            } else if path.extension().map(|e| e == extension).unwrap_or(false) {
                let name = path
                    .strip_prefix(&self.dir)
                    .unwrap_or(&path)
                    .to_string_lossy();
                if !self.partials.contains_key(name.as_ref()) {
                    self.load_internal(&path, Cow::owned(name.to_string()))?;
                }
            }
        }
        Ok(())
    }

    /// Create a new empty aggregator for a given folder. This won't do anything until
    /// a template has been added using [`from_file`](#method.from_file).
    /// ```no_run
    /// # use ramhorns::Ramhorns;
    /// let mut tpls = Ramhorns::lazy("./templates").unwrap();
    /// let content = "I am the content";
    /// let rendered = tpls.from_file("hello.html").unwrap().render(&content);
    /// ```
    pub fn lazy<P: AsRef<Path>>(dir: P) -> Result<Self, Error> {
        Ok(Ramhorns {
            partials: HashMap::default(),
            dir: dir.as_ref().canonicalize()?,
        })
    }

    /// Get the template with the given name, if it exists.
    pub fn get<S>(&self, name: &S) -> Option<&Template<'static>>
    where
        for<'a> Cow<'a, str>: std::borrow::Borrow<S>,
        S: std::hash::Hash + AsRef<Path> + Eq + ?Sized,
    {
        self.partials.get(name)
    }

    /// Get the template with the given name. If the template doesn't exist,
    /// it will be loaded from file and parsed first.
    ///
    /// Use this method in tandem with [`lazy`](#method.lazy).
    pub fn from_file(&mut self, name: &str) -> Result<&Template<'static>, Error> {
        let path = self.dir.join(name);
        if !self.partials.contains_key(name) {
            self.load_internal(&path, Cow::owned(name.to_string()))?;
        }
        Ok(&self.partials[name])
    }

    // Unsafe to expose as it loads the template from arbitrary path.
    #[inline]
    fn load_internal(&mut self, path: &Path, name: Cow<'static, str>) -> Result<(), Error> {
        let file = match std::fs::read_to_string(&path) {
            Ok(file) => Ok(file),
            Err(e) if e.kind() == ErrorKind::NotFound => {
                Err(Error::NotFound(name.to_string().into()))
            }
            Err(e) => Err(Error::Io(e)),
        }?;
        let template = Template::load(file, self)?;
        self.partials.insert(name, template);
        Ok(())
    }
}

pub(crate) trait Partials<'tpl> {
    fn get_partial(&mut self, name: &'tpl str) -> Result<&Template<'tpl>, Error>;
}

impl Partials<'static> for Ramhorns {
    fn get_partial(&mut self, name: &'static str) -> Result<&Template<'static>, Error> {
        if !self.partials.contains_key(name) {
            let path = self.dir.join(name).canonicalize()?;
            if !path.starts_with(&self.dir) {
                return Err(Error::IllegalPartial(name.into()));
            }
            self.load_internal(&path, Cow::borrowed(name))?;
        }
        Ok(&self.partials[name])
    }
}
