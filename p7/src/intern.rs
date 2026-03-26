use std::collections::HashSet;
use std::fmt;
use std::rc::Rc;

/// An interned string — a reference-counted, deduplicated string.
///
/// Cloning is O(1) (just a reference count bump, no heap allocation).
/// Two `InternedString` values from the same interner that represent the same
/// text will point to the same allocation.
#[derive(Clone, Eq, Hash, Ord, PartialOrd)]
pub struct InternedString(Rc<str>);

impl serde::Serialize for InternedString {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(self.as_str())
    }
}

impl<'de> serde::Deserialize<'de> for InternedString {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let s = String::deserialize(deserializer)?;
        Ok(InternedString::from(s))
    }
}

impl InternedString {
    /// Get the underlying string slice.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl std::ops::Deref for InternedString {
    type Target = str;

    fn deref(&self) -> &str {
        &self.0
    }
}

impl PartialEq for InternedString {
    fn eq(&self, other: &Self) -> bool {
        // Fast path: pointer equality (same interned allocation)
        Rc::ptr_eq(&self.0, &other.0) || *self.0 == *other.0
    }
}

impl PartialEq<str> for InternedString {
    fn eq(&self, other: &str) -> bool {
        &*self.0 == other
    }
}

impl PartialEq<&str> for InternedString {
    fn eq(&self, other: &&str) -> bool {
        &*self.0 == *other
    }
}

impl PartialEq<String> for InternedString {
    fn eq(&self, other: &String) -> bool {
        &*self.0 == other.as_str()
    }
}

impl fmt::Debug for InternedString {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Debug::fmt(&*self.0, f)
    }
}

impl fmt::Display for InternedString {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Display::fmt(&*self.0, f)
    }
}

impl AsRef<str> for InternedString {
    fn as_ref(&self) -> &str {
        &self.0
    }
}

impl std::borrow::Borrow<str> for InternedString {
    fn borrow(&self) -> &str {
        &self.0
    }
}

impl From<&str> for InternedString {
    fn from(s: &str) -> Self {
        InternedString(Rc::from(s))
    }
}

impl From<String> for InternedString {
    fn from(s: String) -> Self {
        InternedString(Rc::from(s.as_str()))
    }
}

/// A string interner that deduplicates strings and returns cheap-to-clone handles.
pub struct StringInterner {
    set: HashSet<Rc<str>>,
}

impl Default for StringInterner {
    fn default() -> Self {
        Self::new()
    }
}

impl StringInterner {
    pub fn new() -> Self {
        StringInterner {
            set: HashSet::new(),
        }
    }

    /// Intern a string slice, returning a deduplicated `InternedString`.
    pub fn intern(&mut self, s: &str) -> InternedString {
        if let Some(existing) = self.set.get(s) {
            InternedString(existing.clone())
        } else {
            let rc: Rc<str> = Rc::from(s);
            self.set.insert(rc.clone());
            InternedString(rc)
        }
    }

    /// Intern an owned String, returning a deduplicated `InternedString`.
    /// Avoids double allocation if the string is not yet interned.
    pub fn intern_string(&mut self, s: String) -> InternedString {
        if let Some(existing) = self.set.get(s.as_str()) {
            InternedString(existing.clone())
        } else {
            let rc: Rc<str> = Rc::from(s.as_str());
            self.set.insert(rc.clone());
            InternedString(rc)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_intern_deduplicates() {
        let mut interner = StringInterner::new();
        let a = interner.intern("hello");
        let b = interner.intern("hello");
        assert!(Rc::ptr_eq(&a.0, &b.0));
    }

    #[test]
    fn test_intern_different_strings() {
        let mut interner = StringInterner::new();
        let a = interner.intern("hello");
        let b = interner.intern("world");
        assert!(!Rc::ptr_eq(&a.0, &b.0));
        assert_ne!(a, b);
    }

    #[test]
    fn test_intern_string_owned() {
        let mut interner = StringInterner::new();
        let a = interner.intern("hello");
        let b = interner.intern_string("hello".to_string());
        assert!(Rc::ptr_eq(&a.0, &b.0));
    }

    #[test]
    fn test_clone_is_cheap() {
        let mut interner = StringInterner::new();
        let a = interner.intern("hello");
        let b = a.clone();
        assert!(Rc::ptr_eq(&a.0, &b.0));
    }

    #[test]
    fn test_equality_with_str() {
        let mut interner = StringInterner::new();
        let a = interner.intern("hello");
        assert_eq!(a, "hello");
        assert_eq!(a, *"hello");
        assert_eq!(a, "hello".to_string());
    }

    #[test]
    fn test_deref_to_str() {
        let mut interner = StringInterner::new();
        let a = interner.intern("hello");
        let s: &str = &a;
        assert_eq!(s, "hello");
    }

    #[test]
    fn test_debug_display() {
        let mut interner = StringInterner::new();
        let a = interner.intern("hello");
        assert_eq!(format!("{}", a), "hello");
        assert_eq!(format!("{:?}", a), "\"hello\"");
    }

    #[test]
    fn test_hashmap_key() {
        use std::collections::HashMap;
        let mut interner = StringInterner::new();
        let key = interner.intern("foo");
        let mut map: HashMap<InternedString, i32> = HashMap::new();
        map.insert(key.clone(), 42);
        assert_eq!(map.get(&key), Some(&42));
        // Look up by a separately interned but equal string
        let key2 = interner.intern("foo");
        assert_eq!(map.get(&key2), Some(&42));
    }

    #[test]
    fn test_from_str() {
        let a = InternedString::from("hello");
        assert_eq!(a, "hello");
    }

    #[test]
    fn test_from_string() {
        let a = InternedString::from("hello".to_string());
        assert_eq!(a, "hello");
    }

    #[test]
    fn test_borrow_str_lookup() {
        use std::collections::HashMap;
        let mut interner = StringInterner::new();
        let key = interner.intern("foo");
        let mut map: HashMap<InternedString, i32> = HashMap::new();
        map.insert(key, 42);
        // Look up using &str directly (via Borrow<str>)
        assert_eq!(map.get("foo"), Some(&42));
    }
}
