%{!?okp_upstream_version:%global okp_upstream_version 0.11.0-beta.1}
%{!?okp_rpm_version:%global okp_rpm_version 0.11.0}
%{!?okp_rpm_release:%global okp_rpm_release 0.1.beta.1}

Name:           ok-player
Version:        %{okp_rpm_version}
Release:        %{okp_rpm_release}%{?dist}
Summary:        Native GTK4 media player using Fedora system libraries

License:        GPL-3.0-or-later
URL:            https://github.com/BeFeast/ok-player
Source0:        %{name}-%{okp_upstream_version}.tar.xz
Source1:        %{name}-%{okp_upstream_version}-vendor.tar.xz

ExclusiveArch:  x86_64

BuildRequires:  cargo >= 1.96
BuildRequires:  rust >= 1.96
BuildRequires:  gcc
BuildRequires:  pkgconfig(egl)
BuildRequires:  pkgconfig(gl)
BuildRequires:  pkgconfig(gtk4) >= 4.10
BuildRequires:  pkgconfig(mpv) >= 1.109
BuildRequires:  pkgconfig(wayland-client)
BuildRequires:  pkgconfig(wayland-egl)
BuildRequires:  pkgconfig(x11)
BuildRequires:  appstream
BuildRequires:  desktop-file-utils
BuildRequires:  shared-mime-info

Requires:       mpv-libs%{?_isa}
Requires:       ffmpeg-free

%description
OK Player is a native GTK4 desktop media player built with Fedora's system
media playback libraries.
This Fedora package deliberately uses Fedora's codec set and does not enable or
require third-party repositories. When a codec is unavailable, the player
reports that separately from graphics and application failures.

%prep
%autosetup -n %{name}-%{okp_upstream_version} -a 1
mkdir -p .cargo
cat > .cargo/config.toml <<'EOF'
[source.crates-io]
replace-with = "vendored-sources"

[source.vendored-sources]
directory = "vendor"

[net]
offline = true
EOF

%build
%set_build_flags
export CARGO_HOME="$PWD/.cargo-home"
export CARGO_NET_OFFLINE=true
export CC=%{__cc}
export OKP_BUILD_VERSION=%{okp_upstream_version}
export OKP_PACKAGE_FLAVOR=fedora-native
cargo build \
  --manifest-path rust/Cargo.toml \
  --frozen \
  --release \
  --package okp-linux-gtk

%check
export CARGO_HOME="$PWD/.cargo-home"
export CARGO_NET_OFFLINE=true
export CC=%{__cc}
export OKP_BUILD_VERSION=%{okp_upstream_version}
export OKP_PACKAGE_FLAVOR=fedora-native
cargo test --manifest-path rust/Cargo.toml --frozen --workspace
desktop-file-validate rust/packaging/linux/com.befeast.okplayer.desktop
appstreamcli validate --pedantic --no-color \
  rust/packaging/linux/com.befeast.okplayer.metainfo.xml

%install
install -Dm0755 rust/target/release/okp-linux-gtk \
  %{buildroot}%{_libexecdir}/ok-player/ok-player
mkdir -p %{buildroot}%{_bindir}
ln -s %{_libexecdir}/ok-player/ok-player %{buildroot}%{_bindir}/ok-player

install -Dm0644 rust/packaging/linux/com.befeast.okplayer.desktop \
  %{buildroot}%{_datadir}/applications/com.befeast.okplayer.desktop
install -Dm0644 rust/packaging/linux/com.befeast.okplayer.metainfo.xml \
  %{buildroot}%{_metainfodir}/com.befeast.okplayer.metainfo.xml
install -Dm0644 rust/packaging/linux/com.befeast.okplayer.svg \
  %{buildroot}%{_datadir}/icons/hicolor/scalable/apps/com.befeast.okplayer.svg
for size in 16 24 32 48 64; do
  install -Dm0644 \
    "rust/packaging/linux/icons/hicolor/${size}x${size}/apps/com.befeast.okplayer.svg" \
    "%{buildroot}%{_datadir}/icons/hicolor/${size}x${size}/apps/com.befeast.okplayer.svg"
done

%files
%license LICENSE
%doc THIRD-PARTY-NOTICES.md
%{_bindir}/ok-player
%{_libexecdir}/ok-player/
%{_datadir}/applications/com.befeast.okplayer.desktop
%{_metainfodir}/com.befeast.okplayer.metainfo.xml
%{_datadir}/icons/hicolor/*/apps/com.befeast.okplayer.svg

%changelog
* Fri Jul 17 2026 BeFeast <noreply@github.com> - 0.11.0-0.1.beta.1
- Add the Fedora native beta package lane.
