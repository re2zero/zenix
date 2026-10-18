# RPM spec for zenix — fedora / RHEL / openSUSE.
#
# The binary is pre-built in CI (ubuntu24 glibc container).
# This spec packages the pre-compiled binary and resources.

%global debug_package %{nil}
%define _rpmfilename %%{NAME}-%%{VERSION}-%%{RELEASE}.%%{ARCH}.rpm

Name:    zenix
Version: 0.1.0
Release: 1%{?dist}
Summary: A cross-platform GPUI frontend for herdr terminal workspace manager

License: GPL-3.0-or-later
URL:     https://github.com/re2zero/zenix
Source0: %{name}-%{version}.tar.gz

# Pre-built binary — runtime graphics libs required.
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

# License texts — same inventory as [package.metadata.deb] in Cargo.toml. The
# Lilex faces are compiled into the binary, so no font files ship, but OFL-1.1
# still requires the license text to travel with the font.
%{__install} -Dm644 LICENSE                             %{buildroot}%{_docdir}/zenix/LICENSE
%{__install} -Dm644 README.md                           %{buildroot}%{_docdir}/zenix/README.md
%{__install} -Dm644 THIRD-PARTY.md                      %{buildroot}%{_docdir}/zenix/THIRD-PARTY.md
%{__install} -Dm644 assets/fonts/OFL.txt                %{buildroot}%{_docdir}/zenix/Lilex-OFL.txt
%{__install} -Dm644 herdr/LICENSE                       %{buildroot}%{_docdir}/zenix/herdr/LICENSE
%{__install} -Dm644 vendor/herdr-client/LICENSE-APACHE  %{buildroot}%{_docdir}/zenix/herdr-client/LICENSE-APACHE
%{__install} -Dm644 vendor/herdr-client/NOTICE.md       %{buildroot}%{_docdir}/zenix/herdr-client/NOTICE.md
%{__install} -Dm644 vendor/herdr-protocol/LICENSE-APACHE %{buildroot}%{_docdir}/zenix/herdr-protocol/LICENSE-APACHE
%{__install} -Dm644 vendor/herdr-protocol/NOTICE.md     %{buildroot}%{_docdir}/zenix/herdr-protocol/NOTICE.md

# herdr companion (built by build.rs) — mark ghost so %files doesn't fail if absent
%{__install} -d %{buildroot}%{_datadir}/zenix/
%{__install} -m755 target/release/herdr %{buildroot}%{_datadir}/zenix/herdr 2>/dev/null || true

%files
%{_bindir}/zenix
%dir %{_datadir}/zenix/
%ghost %{_datadir}/zenix/herdr
%{_datadir}/applications/zenix.desktop
%{_iconsdir}/hicolor/512x512/apps/zenix.png
%doc %{_docdir}/zenix/LICENSE
%doc %{_docdir}/zenix/README.md
%doc %{_docdir}/zenix/THIRD-PARTY.md
%doc %{_docdir}/zenix/Lilex-OFL.txt
%doc %{_docdir}/zenix/herdr/LICENSE
%doc %{_docdir}/zenix/herdr-client/LICENSE-APACHE
%doc %{_docdir}/zenix/herdr-client/NOTICE.md
%doc %{_docdir}/zenix/herdr-protocol/LICENSE-APACHE
%doc %{_docdir}/zenix/herdr-protocol/NOTICE.md

%post
update-desktop-database -q 2>/dev/null || :
gtk-update-icon-cache -q -t /usr/share/icons/hicolor 2>/dev/null || :

%postun
update-desktop-database -q 2>/dev/null || :
gtk-update-icon-cache -q -t /usr/share/icons/hicolor 2>/dev/null || :

%changelog
* Fri Jun 13 2025 re2zero <yangwu@uniontech.com>
- Initial RPM package
