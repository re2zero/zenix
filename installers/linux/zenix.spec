# RPM spec for zenix — fedora / RHEL / openSUSE.
#
# The binary is pre-built with musl-static on the Ubuntu runner.
# This spec packages the pre-compiled binary and resources.
#
# Build in CI:
#   rpmbuild -bb --define "version ${VERSION}" \
#     --define "_topdir /tmp/rpmbuild" \
#     installers/linux/zenix.spec

%define _rpmfilename %%{NAME}-%%{VERSION}-%%{RELEASE}.%%{ARCH}.rpm

Name:    zenix
Version: 0.1.0
Release: 1%{?dist}
Summary: A cross-platform GPUI frontend for herdr terminal workspace manager

License: GPL-3.0-or-later
URL:     https://github.com/re2zero/zenix
Source0: %{name}-%{version}.tar.gz

# Static musl binary — no glibc dependency; only runtime graphics libs.
Requires:       libxcb
Requires:       libxkbcommon
Requires:       fontconfig
Requires:       freetype
Requires:       wayland
Requires:       mesa-libEGL
Requires:       mesa-libGL
Requires:       desktop-file-utils
Requires:       gtk-update-icon-cache

BuildArch: x86_64

%description
zenix is an extensible terminal workspace manager designed for AI coding agents.
It provides workspace, tab, and pane management with a client-server
architecture, plus system monitoring and plugin support.

%prep
%setup -q

%build
# Pre-built in CI; skip build.  For local builds: cargo build --release
%{nil}

%install
%{__install} -Dm755 target/release/zenix                %{buildroot}%{_bindir}/zenix
%{__install} -Dm644 res/zenix.desktop                   %{buildroot}%{_datadir}/applications/zenix.desktop
%{__install} -Dm644 res/zenix.png                       %{buildroot}%{_datadir}/icons/hicolor/512x512/apps/zenix.png
%{__install} -Dm644 LICENSE                             %{buildroot}%{_docdir}/zenix/LICENSE
%{__install} -Dm644 README.md                           %{buildroot}%{_docdir}/zenix/README.md

# herdr companion (built by build.rs) — mark ghost so %files doesn't fail if absent
%{__install} -d %{buildroot}%{_datadir}/zenix/
%{__install} -m755 target/release/herdr %{buildroot}%{_datadir}/zenix/herdr 2>/dev/null || true

# Fonts
%{__install} -Dm644 assets/fonts/Lilex-Regular.ttf      %{buildroot}%{_datadir}/zenix/fonts/Lilex-Regular.ttf
%{__install} -Dm644 assets/fonts/Lilex-Bold.ttf         %{buildroot}%{_datadir}/zenix/fonts/Lilex-Bold.ttf
%{__install} -Dm644 assets/fonts/Lilex-Italic.ttf       %{buildroot}%{_datadir}/zenix/fonts/Lilex-Italic.ttf
%{__install} -Dm644 assets/fonts/Lilex-BoldItalic.ttf   %{buildroot}%{_datadir}/zenix/fonts/Lilex-BoldItalic.ttf

# Themes
%{__install} -Dm644 assets/themes/gruvbox.json          %{buildroot}%{_datadir}/zenix/themes/gruvbox.json
%{__install} -Dm644 assets/themes/solarized.json        %{buildroot}%{_datadir}/zenix/themes/solarized.json
%{__install} -Dm644 assets/themes/tokyonight.json       %{buildroot}%{_datadir}/zenix/themes/tokyonight.json
%{__install} -Dm644 assets/themes/matrix.json           %{buildroot}%{_datadir}/zenix/themes/matrix.json

%files
%{_bindir}/zenix
%dir %{_datadir}/zenix/
%ghost %{_datadir}/zenix/herdr
%{_datadir}/applications/zenix.desktop
%{_iconsdir}/hicolor/512x512/apps/zenix.png
%{_docdir}/zenix/LICENSE
%{_docdir}/zenix/README.md
%{_datadir}/zenix/fonts/*
%{_datadir}/zenix/themes/*

%post
update-desktop-database -q 2>/dev/null || :
gtk-update-icon-cache -q -t /usr/share/icons/hicolor 2>/dev/null || :

%postun
update-desktop-database -q 2>/dev/null || :
gtk-update-icon-cache -q -t /usr/share/icons/hicolor 2>/dev/null || :

%changelog
* 2025-06-13  re2zero <yangwu@uniontech.com>
- Initial RPM package
