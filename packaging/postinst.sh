#!/bin/sh
# .msg dosya türü ve masaüstü dosyası önbelleklerini hemen güncelle (tetikleyici
# beklenmeden): .msg dosyaları kurulumdan hemen sonra Zarf ile açılabilsin
command -v update-mime-database >/dev/null 2>&1 && update-mime-database /usr/share/mime >/dev/null 2>&1 || true
command -v update-desktop-database >/dev/null 2>&1 && update-desktop-database -q /usr/share/applications >/dev/null 2>&1 || true
# Yeni uygulama simgesi hemen görünsün (simge önbelleği tazelenir)
command -v gtk-update-icon-cache >/dev/null 2>&1 && gtk-update-icon-cache -q -t -f /usr/share/icons/hicolor >/dev/null 2>&1 || true
exit 0
