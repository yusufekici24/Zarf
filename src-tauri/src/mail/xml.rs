//! Basit XML ağacı (ad alanları göz ardı edilir) — EWS/Autodiscover yanıtları için.

use quick_xml::events::Event;
use quick_xml::Reader;

#[derive(Debug, Clone, Default)]
pub struct Element {
    pub name: String,
    pub attrs: Vec<(String, String)>,
    pub children: Vec<Element>,
    pub text: String,
}

impl Element {
    pub fn attr(&self, name: &str) -> Option<&str> {
        self.attrs.iter().find(|(k, _)| k == name).map(|(_, v)| v.as_str())
    }

    pub fn child(&self, name: &str) -> Option<&Element> {
        self.children.iter().find(|c| c.name == name)
    }

    pub fn children_named<'a>(&'a self, name: &'a str) -> impl Iterator<Item = &'a Element> + 'a {
        self.children.iter().filter(move |c| c.name == name)
    }

    /// Tüm alt düğümlerde (derinlemesine) ilk eşleşme
    pub fn find(&self, name: &str) -> Option<&Element> {
        for c in &self.children {
            if c.name == name {
                return Some(c);
            }
            if let Some(f) = c.find(name) {
                return Some(f);
            }
        }
        None
    }

    pub fn find_all<'a>(&'a self, name: &str, out: &mut Vec<&'a Element>) {
        for c in &self.children {
            if c.name == name {
                out.push(c);
            }
            c.find_all(name, out);
        }
    }

    /// "A/B/C" yolundaki metin
    pub fn path_text(&self, path: &str) -> Option<String> {
        let mut cur = self;
        for p in path.split('/') {
            cur = cur.child(p)?;
        }
        Some(cur.text.clone())
    }

    /// Öğeyi (tüm alt öğeleriyle) verilen önekle XML'e geri yazar.
    pub fn to_xml(&self, prefix: &str) -> String {
        let mut out = format!("<{prefix}:{}", self.name);
        for (k, v) in &self.attrs {
            if !k.contains(':') && k != "xmlns" {
                out.push_str(&format!(" {k}=\"{}\"", esc(v)));
            }
        }
        out.push('>');
        if self.children.is_empty() {
            out.push_str(&esc(&self.text));
        }
        for c in &self.children {
            out.push_str(&c.to_xml(prefix));
        }
        out.push_str(&format!("</{prefix}:{}>", self.name));
        out
    }

    pub fn text_of(&self, name: &str) -> String {
        self.child(name).map(|c| c.text.clone()).unwrap_or_default()
    }
}

fn local(name: &str) -> String {
    name.rsplit(':').next().unwrap_or(name).to_string()
}

/// Öğe adı ve öznitelikleri (öznitelik değerlerindeki varlıklar çözülür)
fn element(e: &quick_xml::events::BytesStart) -> Element {
    let mut el = Element { name: local(e.name().as_ref()), ..Default::default() };
    for a in e.attributes().flatten() {
        let raw = a.value.to_string();
        let v = quick_xml::escape::unescape(&raw).map(|c| c.to_string()).unwrap_or(raw);
        el.attrs.push((local(a.key.as_ref()), v));
    }
    el
}

/// "&amp;", "&#252;" gibi varlık başvuruları
fn entity(name: &str) -> Option<String> {
    if let Some(n) = name.strip_prefix('#') {
        let code = match n.strip_prefix('x').or_else(|| n.strip_prefix('X')) {
            Some(h) => u32::from_str_radix(h, 16).ok()?,
            None => n.parse().ok()?,
        };
        return char::from_u32(code).map(String::from);
    }
    quick_xml::escape::resolve_xml_entity(name).map(String::from)
}

pub fn parse(data: &[u8]) -> Result<Element, String> {
    let mut reader = Reader::from_reader(data);
    let mut stack: Vec<Element> = vec![Element { name: "#root".into(), ..Default::default() }];
    let mut buf = Vec::new();
    loop {
        match reader.read_event_into(&mut buf) {
            Ok(Event::Start(e)) => stack.push(element(&e)),
            Ok(Event::Empty(e)) => {
                let el = element(&e);
                stack.last_mut().unwrap().children.push(el);
            }
            Ok(Event::Text(t)) => {
                // Ham içerik: satır sonları olduğu gibi kalır (önceki sürümle aynı)
                stack.last_mut().unwrap().text.push_str(&t);
            }
            Ok(Event::GeneralRef(r)) => {
                let name = r.to_string();
                let s = entity(&name).unwrap_or_else(|| format!("&{name};"));
                stack.last_mut().unwrap().text.push_str(&s);
            }
            Ok(Event::CData(t)) => stack.last_mut().unwrap().text.push_str(&t),
            Ok(Event::End(_)) => {
                if stack.len() > 1 {
                    let el = stack.pop().unwrap();
                    stack.last_mut().unwrap().children.push(el);
                }
            }
            Ok(Event::Eof) => break,
            Ok(_) => {}
            Err(e) => return Err(format!("XML: {e}")),
        }
        buf.clear();
    }
    while stack.len() > 1 {
        let el = stack.pop().unwrap();
        stack.last_mut().unwrap().children.push(el);
    }
    Ok(stack.pop().unwrap())
}

pub fn esc(s: &str) -> String {
    s.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;").replace('"', "&quot;").replace('\'', "&apos;")
}

#[cfg(test)]
mod tests {
    #[test]
    fn parse_ns() {
        let x = br#"<?xml version="1.0"?><s:Envelope xmlns:s="x"><s:Body><m:R a="1"><t:Id Id="AAA=" ChangeKey="c"/><t:Name>Gelen &amp; Kutusu</t:Name></m:R></s:Body></s:Envelope>"#;
        let r = super::parse(x).unwrap();
        let id = r.find("Id").unwrap();
        assert_eq!(id.attr("Id"), Some("AAA="));
        assert_eq!(r.find("Name").unwrap().text, "Gelen & Kutusu");
        assert_eq!(r.find("R").unwrap().attr("a"), Some("1"));
        let y = br#"<a t="x &amp; &#252;">&#x130;stanbul &lt;b&gt; &#287;</a>"#;
        let r = super::parse(y).unwrap();
        assert_eq!(r.find("a").unwrap().text, "İstanbul <b> ğ");
        assert_eq!(r.find("a").unwrap().attr("t"), Some("x & ü"));
        // Satır sonları korunur (takvim açıklaması, imza)
        let z = b"<b>a\r\nb</b>";
        assert_eq!(super::parse(z).unwrap().find("b").unwrap().text, "a\r\nb");
    }
}
