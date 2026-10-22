Name:           scythe
Version:        0.2.22
Release:        1%{?dist}
Summary:        High-performance GPU hardware screen recorder and instant replay overlay

License:        MIT
URL:            https://github.com/oiupoyt/scythe
Source0:        %{name}-%{version}.tar.gz

BuildRequires:  cargo
BuildRequires:  rust
BuildRequires:  ffmpeg-devel
BuildRequires:  pipewire-devel
BuildRequires:  libva-devel
BuildRequires:  alsa-lib-devel
BuildRequires:  libxcb-devel
BuildRequires:  clang-devel
BuildRequires:  pkgconf-pkg-config

Requires:       ffmpeg-free
Requires:       pipewire
Requires:       libva
Requires:       alsa-lib
Requires:       libxcb

%description
High-performance GPU hardware screen recorder and instant replay overlay for Wayland and X11.

%prep
%autosetup

%build
cargo build --release

%install
rm -rf $RPM_BUILD_ROOT
install -D -m 0755 target/release/scythe %{buildroot}%{_bindir}/scythe
ln -sf %{_bindir}/scythe %{buildroot}%{_bindir}/scythe-daemon
ln -sf %{_bindir}/scythe %{buildroot}%{_bindir}/scythe-ui
ln -sf %{_bindir}/scythe %{buildroot}%{_bindir}/vrec-daemon
ln -sf %{_bindir}/scythe %{buildroot}%{_bindir}/vrec-ui
ln -sf %{_bindir}/scythe %{buildroot}%{_bindir}/vrec
install -D -m 0644 packaging/scythe.desktop %{buildroot}%{_datadir}/applications/scythe.desktop
ln -sf %{_datadir}/applications/scythe.desktop %{buildroot}%{_datadir}/applications/vrec.desktop

%files
%{_bindir}/scythe
%{_bindir}/scythe-daemon
%{_bindir}/scythe-ui
%{_bindir}/vrec-daemon
%{_bindir}/vrec-ui
%{_bindir}/vrec
%{_datadir}/applications/scythe.desktop
%{_datadir}/applications/vrec.desktop

%changelog
* Tue Oct 21 2026 oiupoyt - 0.2.21-1
- Unified multicall binary: install scythe with the scythe-daemon, scythe-ui
  and vrec-* compatibility symlinks
- Dropped the GTK 3 and GTK layer shell requirements, the GTK backend was
  retired
- Corrected the runtime dependency list against the actual build output
- Version tracked against Cargo.toml
* Wed Sep 02 2026 oiupoyt - 0.1.0-1
- Initial release of scythe
