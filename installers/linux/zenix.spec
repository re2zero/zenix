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
#
# The .deb declares `depends = "$auto, libegl1, libwayland-client0"`. `$auto` is
# cargo-deb's ldd-derived list, and rpm derives the same list on its own from the
# ELF DT_NEEDED entries, so only what ldd cannot see (the dlopen'd EGL and
# Wayland client libraries, per Cargo.toml's comment) belongs here explicitly —
# that is mesa-libEGL for libegl1. The rest is the case both ecosystems share.
# Fedora ships no binary package named `wayland`; the soname's provider is
# libwayland-client (libwayland-client0 on openSUSE). There is no
# `Requires: mesa-libGL`: the .deb's `$auto` never found a libGL dependency
# either, because wgpu's GLES path reaches GL through EGL.
Requires:       libxcb
Requires:       libxkbcommon
Requires:       fontconfig
Requires:       freetype
Requires:       libwayland-client
Requires:       mesa-libEGL
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

# The daemon seed: the copy the client installs to ~/.local/bin/herdr on first
# run (src/platform.rs::seed_herdr reads %{_datadir}/zenix/herdr). build.rs makes
# one binary and writes it twice — target/<profile>/herdr and the res/herdr seed
# the .deb ships — and the CI source tarball carries target/release/herdr. No
# `|| true`: a package without the seed installs a client that cannot connect,
# so a missing seed has to fail the build (same rule as the .deb's asset list).
%{__install} -Dm755 target/release/herdr                %{buildroot}%{_datadir}/zenix/herdr

# License texts — same inventory as [package.metadata.deb] in Cargo.toml, which
# is the authoritative list. Only the project license diverges by name, because
# each ecosystem has its own convention for it:
#   .deb  usr/share/doc/zenix/copyright   (cargo-deb)
#   .rpm  %{_docdir}/zenix/LICENSE        (Fedora)
# The Lilex faces are compiled into the binary, so no font files ship, but
# OFL-1.1 still requires the license text to travel with the font.
%{__install} -Dm644 LICENSE                             %{buildroot}%{_docdir}/zenix/LICENSE
%{__install} -Dm644 THIRD-PARTY.md                      %{buildroot}%{_docdir}/zenix/THIRD-PARTY.md
%{__install} -Dm644 assets/fonts/OFL.txt                %{buildroot}%{_docdir}/zenix/Lilex-OFL.txt
%{__install} -Dm644 herdr/LICENSE                       %{buildroot}%{_docdir}/zenix/herdr/LICENSE
%{__install} -Dm644 vendor/herdr-client/LICENSE-APACHE  %{buildroot}%{_docdir}/zenix/herdr-client/LICENSE-APACHE
%{__install} -Dm644 vendor/herdr-client/NOTICE.md       %{buildroot}%{_docdir}/zenix/herdr-client/NOTICE.md
%{__install} -Dm644 vendor/herdr-protocol/LICENSE-APACHE %{buildroot}%{_docdir}/zenix/herdr-protocol/LICENSE-APACHE
%{__install} -Dm644 vendor/herdr-protocol/NOTICE.md     %{buildroot}%{_docdir}/zenix/herdr-protocol/NOTICE.md

%files
# Compared entry by entry against `dpkg-deb -c target/debian/*.deb`; the only
# intentional differences are the project license name (see %install) and the
# absence of a README (the .deb ships none, so neither does this).
%{_bindir}/zenix
%dir %{_datadir}/zenix/
%{_datadir}/zenix/herdr
%{_datadir}/applications/zenix.desktop
%{_iconsdir}/hicolor/512x512/apps/zenix.png
%dir %{_docdir}/zenix/
%doc %{_docdir}/zenix/LICENSE
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
