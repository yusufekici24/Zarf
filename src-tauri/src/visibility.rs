//! MailNix kullanıcının önünde mi: yeni posta uyarısı yalnızca uygulama görünmüyorken
//! (simge durumunda, başka masaüstünde ya da başka pencerelerin arkasında kalmışken) gösterilir.
//! Odak başka bir uygulamadayken (ör. ikinci monitörde tarayıcı) MailNix penceresi ekranda
//! açık duruyorsa uyarı gelmez.
//!
//! X11: pencere yöneticisinin üst üste diziliş sırası (_NET_CLIENT_LIST_STACKING) okunur ve
//! MailNix penceresinin üstündeki pencerelerin onu ne kadar örttüğü ölçülür.
//! Wayland (XWayland yoksa): diziliş bilinemez; odaklı bir MailNix penceresi varsa görünür sayılır.

use tauri::{AppHandle, Manager};

/// Pencerenin bu oranından fazlası örtülüyse görünmüyor sayılır
const COVERED_LIMIT: f64 = 0.6;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Rect {
    pub x: i32,
    pub y: i32,
    pub w: i32,
    pub h: i32,
}

impl Rect {
    fn contains(&self, px: i32, py: i32) -> bool {
        px >= self.x && py >= self.y && px < self.x + self.w && py < self.y + self.h
    }
}

/// `target` alanının `above` dikdörtgenlerince örtülen oranı (ızgara örneklemesiyle)
pub fn covered_fraction(target: Rect, above: &[Rect]) -> f64 {
    if target.w <= 0 || target.h <= 0 {
        return 1.0;
    }
    const N: i32 = 24;
    let mut hit = 0;
    for i in 0..N {
        for j in 0..N {
            let px = target.x + (2 * i + 1) * target.w / (2 * N);
            let py = target.y + (2 * j + 1) * target.h / (2 * N);
            if above.iter().any(|r| r.contains(px, py)) {
                hit += 1;
            }
        }
    }
    hit as f64 / (N * N) as f64
}

/// Uyarı penceresi dışındaki MailNix pencereleri (gizli yedek pencere hariç)
fn app_windows(app: &AppHandle) -> Vec<tauri::WebviewWindow> {
    app.webview_windows().into_values().filter(|w| w.label() != "alerts" && w.label() != "reminders" && w.is_visible().unwrap_or(false)).collect()
}

/// Kullanıcı şu an bir MailNix penceresini görebiliyor mu
#[tauri::command]
pub fn app_visible(app: AppHandle) -> bool {
    let wins = app_windows(&app);
    if wins.is_empty() {
        return false;
    }
    if wins.iter().any(|w| w.is_focused().unwrap_or(false)) {
        return true;
    }
    #[cfg(target_os = "linux")]
    if let Some(v) = x11::visible(&wins) {
        return v;
    }
    // Diziliş bilinemiyor (Wayland): odak yoksa görünmüyor say (uyarı kaçmasın)
    false
}

#[cfg(target_os = "linux")]
mod x11 {
    use super::{covered_fraction, Rect, COVERED_LIMIT};
    use std::ffi::CString;
    use std::os::raw::{c_int, c_long, c_uchar, c_ulong};
    use x11_dl::xlib;

    /// GTK penceresinin X11 kimliği (Wayland'de yok)
    fn xid(w: &tauri::WebviewWindow) -> Option<c_ulong> {
        use gdkx11::X11Window;
        use gtk::glib::Cast;
        use gtk::prelude::WidgetExt;
        let gw = w.gtk_window().ok()?;
        let gdk = gw.window()?;
        gdk.downcast::<X11Window>().ok().map(|x| x.xid() as c_ulong)
    }

    struct Display<'a> {
        x: &'a xlib::Xlib,
        d: *mut xlib::Display,
    }

    impl Drop for Display<'_> {
        fn drop(&mut self) {
            unsafe { (self.x.XCloseDisplay)(self.d) };
        }
    }

    impl Display<'_> {
        fn atom(&self, name: &str) -> xlib::Atom {
            let c = CString::new(name).unwrap();
            unsafe { (self.x.XInternAtom)(self.d, c.as_ptr(), xlib::False) }
        }

        /// 32 bitlik özellik değerleri (pencere listesi, durum, masaüstü numarası)
        fn prop(&self, win: c_ulong, name: &str) -> Vec<c_ulong> {
            let mut actual_type = 0;
            let mut format: c_int = 0;
            let mut n: c_ulong = 0;
            let mut after: c_ulong = 0;
            let mut data: *mut c_uchar = std::ptr::null_mut();
            let ok = unsafe {
                (self.x.XGetWindowProperty)(self.d, win, self.atom(name), 0, 4096, xlib::False, xlib::AnyPropertyType as c_ulong, &mut actual_type, &mut format, &mut n, &mut after, &mut data)
            };
            if ok != xlib::Success as c_int || data.is_null() {
                return vec![];
            }
            // Xlib 32 bitlik değerleri long dizisi olarak verir
            let v = if format == 32 { unsafe { std::slice::from_raw_parts(data as *const c_long, n as usize) }.iter().map(|x| *x as c_ulong).collect() } else { vec![] };
            unsafe { (self.x.XFree)(data as *mut _) };
            v
        }

        /// Ekrandaki konum ve boyut; görünür değilse None
        fn rect(&self, root: c_ulong, win: c_ulong) -> Option<Rect> {
            let mut a: xlib::XWindowAttributes = unsafe { std::mem::zeroed() };
            if unsafe { (self.x.XGetWindowAttributes)(self.d, win, &mut a) } == 0 || a.map_state != xlib::IsViewable {
                return None;
            }
            let (mut x, mut y, mut child) = (0, 0, 0);
            unsafe { (self.x.XTranslateCoordinates)(self.d, win, root, 0, 0, &mut x, &mut y, &mut child) };
            Some(Rect { x, y, w: a.width, h: a.height })
        }
    }

    /// Some(true/false): X11'de ölçüldü; None: X11 değil (ya da okunamadı)
    pub fn visible(wins: &[tauri::WebviewWindow]) -> Option<bool> {
        let ours: Vec<c_ulong> = wins.iter().filter_map(xid).collect();
        if ours.is_empty() {
            return None;
        }
        let x = xlib::Xlib::open().ok()?;
        let d = unsafe { (x.XOpenDisplay)(std::ptr::null()) };
        if d.is_null() {
            return None;
        }
        let disp = Display { x: &x, d };
        let root = unsafe { (x.XDefaultRootWindow)(d) };
        let stacking = disp.prop(root, "_NET_CLIENT_LIST_STACKING");
        if stacking.is_empty() {
            return None;
        }
        let current = disp.prop(root, "_NET_CURRENT_DESKTOP").first().copied();
        let hidden = disp.atom("_NET_WM_STATE_HIDDEN");
        // Bu masaüstünde ekranda görünen pencereler (alttan üste)
        let shown: Vec<(c_ulong, Rect)> = stacking
            .iter()
            .filter_map(|&w| {
                if disp.prop(w, "_NET_WM_STATE").contains(&hidden) {
                    return None;
                }
                let desk = disp.prop(w, "_NET_WM_DESKTOP").first().copied();
                if let (Some(c), Some(dk)) = (current, desk) {
                    if dk != c && dk != 0xFFFF_FFFF {
                        return None;
                    }
                }
                disp.rect(root, w).map(|r| (w, r))
            })
            .collect();
        for (i, (w, r)) in shown.iter().enumerate() {
            if !ours.contains(w) {
                continue;
            }
            // Üstündeki başka uygulamaların pencereleri (kendi pencerelerimiz örtmez)
            let above: Vec<Rect> = shown[i + 1..].iter().filter(|(o, _)| !ours.contains(o)).map(|(_, r)| *r).collect();
            if covered_fraction(*r, &above) < COVERED_LIMIT {
                return Some(true);
            }
        }
        Some(false)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn coverage() {
        let me = Rect { x: 1920, y: 0, w: 1000, h: 800 };
        // İkinci monitördeki tarayıcı MailNix'i örtmez
        assert_eq!(covered_fraction(me, &[Rect { x: 0, y: 0, w: 1920, h: 1080 }]), 0.0);
        // Tam üstünde ekranı kaplayan pencere
        assert_eq!(covered_fraction(me, &[Rect { x: 1920, y: 0, w: 1920, h: 1080 }]), 1.0);
        // Yarısı örtülü: görünür sayılır
        let half = covered_fraction(me, &[Rect { x: 1920, y: 0, w: 500, h: 800 }]);
        assert!((half - 0.5).abs() < 0.05 && half < COVERED_LIMIT);
        // İki pencere birlikte çoğunu örtüyor
        assert!(covered_fraction(me, &[Rect { x: 1920, y: 0, w: 600, h: 800 }, Rect { x: 2400, y: 0, w: 600, h: 800 }]) > COVERED_LIMIT);
    }
}
