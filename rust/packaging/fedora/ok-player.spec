# rpm_version is upstream_version with every `-` replaced by `~`, the rule
# scripts/linux-package-version.sh owns for both Linux package lanes (issue #709).
# scripts/package-linux-rpm-source.sh always passes both, so these two defaults only apply to
# a bare `rpmbuild` — and they are a matched pair, pinned by
# scripts/tests/linux-package-version.Tests.sh so they cannot drift apart.
%{!?upstream_version:%global upstream_version 0.11.0-beta.1}
%{!?rpm_version:%global rpm_version 0.11.0~beta.1}
%{!?rpm_release:%global rpm_release 1}

Name:           ok-player
Version:        %{rpm_version}
Release:        %{rpm_release}%{?dist}
Summary:        Native GTK4 media player
License:        GPL-3.0-or-later
URL:            https://github.com/BeFeast/ok-player
Source0:        %{name}-%{upstream_version}.tar.gz
Source1:        %{name}-%{upstream_version}-vendor.tar.zst
Source2:        %{name}-%{upstream_version}-source-commit

ExclusiveArch:  x86_64

BuildRequires:  cargo >= 1.96
BuildRequires:  rust >= 1.96
BuildRequires:  gcc
BuildRequires:  pkgconfig
BuildRequires:  pkgconfig(gtk4) >= 4.10
BuildRequires:  pkgconfig(mpv) >= 1.109
BuildRequires:  pkgconfig(x11)
BuildRequires:  pkgconfig(wayland-client)
BuildRequires:  pkgconfig(wayland-egl)
BuildRequires:  wayland-protocols-devel
BuildRequires:  pkgconfig(egl)
BuildRequires:  pkgconfig(gl)
BuildRequires:  desktop-file-utils
BuildRequires:  appstream
BuildRequires:  zstd
BuildRequires:  jq
BuildRequires:  dbus-daemon
BuildRequires:  python3
BuildRequires:  procps-ng

Requires:       mpv-libs%{?_isa}
Requires:       xdg-utils
# GTK4 reaches OpenGL ES through libepoxy's dlopen, so the binary has no
# DT_NEEDED entry for it and rpm's automatic dependency extraction cannot see
# it. A root that satisfies only the linked closure has no libGLESv2.so.2 and
# the application aborts during startup with
# "Couldn't open libGLESv2.so.2". Declare the dlopen'ed soname explicitly so
# dnf pulls a provider (libglvnd-gles on current Fedora) with the package.
Requires:       libGLESv2.so.2()(64bit)

%description
OK Player is a native GTK4 desktop media player. The Fedora package dynamically
links Fedora's system playback libraries and uses the codec support provided by
the enabled Fedora repositories.

%prep
%autosetup -n %{name}-%{upstream_version} -a 1
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
export CARGO_HOME="%{_builddir}/cargo-home"
export OKP_BUILD_VERSION="%{upstream_version}"
export OKP_BUILD_SHA="$(cat %{SOURCE2})"
export OKP_PACKAGE_KIND=rpm
export OKP_FEDORA_RPM=1
export OKP_REQUIRE_SYSTEM_MPV=1
CC=/usr/bin/cc cargo build \
  --manifest-path rust/Cargo.toml \
  --frozen \
  --release \
  -p okp-linux-gtk \
  --bin okp-linux-gtk

%install
install -Dm0755 rust/target/release/okp-linux-gtk \
  %{buildroot}%{_bindir}/ok-player
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
install -Dm0644 LICENSE %{buildroot}%{_licensedir}/%{name}/LICENSE
install -Dm0644 THIRD-PARTY-NOTICES.md \
  %{buildroot}%{_docdir}/%{name}/THIRD-PARTY-NOTICES.md

%check
export CARGO_HOME="%{_builddir}/cargo-home"
export OKP_BUILD_VERSION="%{upstream_version}"
export OKP_BUILD_SHA="$(cat %{SOURCE2})"
export OKP_PACKAGE_KIND=rpm
export OKP_FEDORA_RPM=1
export OKP_REQUIRE_SYSTEM_MPV=1
CC=/usr/bin/cc cargo test \
  --manifest-path rust/Cargo.toml \
  --frozen \
  --workspace \
  --all-targets
desktop-file-validate \
  %{buildroot}%{_datadir}/applications/com.befeast.okplayer.desktop
appstreamcli validate --no-net --pedantic \
  %{buildroot}%{_metainfodir}/com.befeast.okplayer.metainfo.xml

%files
%license %{_licensedir}/%{name}/LICENSE
%doc %{_docdir}/%{name}/THIRD-PARTY-NOTICES.md
%{_bindir}/ok-player
%{_datadir}/applications/com.befeast.okplayer.desktop
%{_metainfodir}/com.befeast.okplayer.metainfo.xml
%{_datadir}/icons/hicolor/*/apps/com.befeast.okplayer.svg

%changelog
* Fri Jul 17 2026 BeFeast <noreply@github.com> - 0.11.0~beta.1-1
- Add the native Fedora beta package.
