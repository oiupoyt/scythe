Name:           scythe
Version:        0.1.0
Release:        1%{?dist}
Summary:        High-performance GPU hardware screen recorder and instant replay overlay

License:        MIT
URL:            https://github.com/oiupoyt/scythe
Source0:        %{name}-%{version}.tar.gz

BuildRequires:  cargo
BuildRequires:  rust
BuildRequires:  ffmpeg-devel
BuildRequires:  pipewire-devel
BuildRequires:  gtk3-devel
BuildRequires:  gtk-layer-shell-devel
BuildRequires:  libxcb-devel
BuildRequires:  clang-devel

Requires:       ffmpeg-free
Requires:       pipewire
Requires:       gtk3
Requires:       gtk-layer-shell

%description
High-performance GPU hardware screen recorder and instant replay overlay for Wayland and X11.

%prep
%autosetup

%build
cargo build --release

%install
rm -rf $RPM_BUILD_ROOT
install -D -m 0755 target/release/scythe-daemon %{buildroot}%{_bindir}/scythe-daemon
install -D -m 0755 target/release/scythe-ui %{buildroot}%{_bindir}/scythe-ui
install -D -m 0755 target/release/scythe %{buildroot}%{_bindir}/scythe
ln -sf %{_bindir}/scythe-daemon %{buildroot}%{_bindir}/vrec-daemon
ln -sf %{_bindir}/scythe-ui %{buildroot}%{_bindir}/vrec-ui
ln -sf %{_bindir}/scythe %{buildroot}%{_bindir}/vrec
install -D -m 0644 packaging/scythe.desktop %{buildroot}%{_datadir}/applications/scythe.desktop
ln -sf %{_datadir}/applications/scythe.desktop %{buildroot}%{_datadir}/applications/vrec.desktop

%files
%{_bindir}/scythe-daemon
%{_bindir}/scythe-ui
%{_bindir}/scythe
%{_bindir}/vrec-daemon
%{_bindir}/vrec-ui
%{_bindir}/vrec
%{_datadir}/applications/scythe.desktop
%{_datadir}/applications/vrec.desktop

%changelog
* Wed Sep 02 2026 oiupoyt - 0.1.0-1
- Initial release of scythe
