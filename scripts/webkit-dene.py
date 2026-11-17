#!/usr/bin/env python3
"""Ön yüzü gerçek WebKitGTK (4.1) içinde, ekranda görünmeyen bir pencerede açar; fare olayı üretir,
sayfada betik çalıştırır ve görüntü alır. Playwright Chrome kullanır; motora özgü davranışlar (ör. fare
basılıyken :hover'ın güncellenmemesi) burada denenir. Uygulama derlenmez, sudo gerekmez.

  npm run dev                                            # ayrı uçbirimde (127.0.0.1:1420)
  node scripts/webkit-mock.mjs /tmp/init.js '{"theme":"light"}'
  GDK_BACKEND=x11 python3 scripts/webkit-dene.py /tmp/init.js /tmp/cikti ADIM...

Adımlar sırayla çalışır:
  shot:ad        /tmp/cikti-ad.png yazar          move:x,y | down:x,y | up:x,y   fare (sol düğme)
  wait:ms        bekler                           js:ifade                       sonucu "js>" ile yazar
Gerekenler: python3-gi, gir1.2-gtk-3.0, gir1.2-webkit2-4.1. Pencere 1400x900'dür.
"""
import sys

import gi

gi.require_version("Gtk", "3.0")
gi.require_version("Gdk", "3.0")
gi.require_version("WebKit2", "4.1")
from gi.repository import Gdk, GLib, Gtk, WebKit2  # noqa: E402

if len(sys.argv) < 4:
    sys.exit(__doc__)
init_js, prefix, steps = sys.argv[1], sys.argv[2], sys.argv[3:]

ucm = WebKit2.UserContentManager()
with open(init_js, encoding="utf-8") as fh:
    ucm.add_script(WebKit2.UserScript.new(fh.read(), WebKit2.UserContentInjectedFrames.TOP_FRAME, WebKit2.UserScriptInjectionTime.START, None, None))
view = WebKit2.WebView.new_with_user_content_manager(ucm)
win = Gtk.OffscreenWindow()
win.set_default_size(1400, 900)
win.add(view)
win.show_all()
print("webkit", WebKit2.get_major_version(), WebKit2.get_minor_version(), WebKit2.get_micro_version(), flush=True)

pointer = Gdk.Display.get_default().get_default_seat().get_pointer()
held = 0


def mouse(kind: str, x: float, y: float) -> None:
    """Gerçek imleci oynatmadan web görünümüne fare olayı gönderir."""
    global held
    ev = Gdk.Event.new({"move": Gdk.EventType.MOTION_NOTIFY, "down": Gdk.EventType.BUTTON_PRESS, "up": Gdk.EventType.BUTTON_RELEASE}[kind])
    ev.window = view.get_window()
    ev.send_event = True
    ev.time = Gdk.CURRENT_TIME
    ev.x = ev.x_root = x
    ev.y = ev.y_root = y
    ev.set_device(pointer)
    ev.state = Gdk.ModifierType(held)
    if kind != "move":
        ev.button = 1
        held = int(Gdk.ModifierType.BUTTON1_MASK) if kind == "down" else 0
    view.event(ev)


def run(i: int = 0) -> bool:
    if i >= len(steps):
        Gtk.main_quit()
        return False
    op, _, arg = steps[i].partition(":")

    def then(ms: int = 100) -> bool:
        GLib.timeout_add(ms, run, i + 1)
        return False

    if op == "wait":
        return then(int(arg))
    if op in ("move", "down", "up"):
        x, y = arg.split(",")
        mouse(op, float(x), float(y))
        return then(250)
    if op == "shot":
        win.get_pixbuf().savev(f"{prefix}-{arg}.png", "png", [], [])
        print("shot", f"{prefix}-{arg}.png", flush=True)
        return then()
    if op == "js":
        def done(v, res):
            try:
                print("js>", v.evaluate_javascript_finish(res).to_string(), flush=True)
            except GLib.Error as e:
                print("js!", e.message, flush=True)
            then()

        view.evaluate_javascript(arg, -1, None, None, None, done)
        return False
    sys.exit(f"bilinmeyen adım: {steps[i]}")


def loaded(_view, event) -> None:
    if event == WebKit2.LoadEvent.FINISHED:
        GLib.timeout_add(3500, run, 0)


view.connect("load-changed", loaded)
view.load_uri("http://127.0.0.1:1420/")
GLib.timeout_add_seconds(120, Gtk.main_quit)
Gtk.main()
