VERSION ?= 0.11.0-beta.1
RPM_RELEASE ?= 0.1.beta.1

.PHONY: srpm
srpm:
	dnf -y install cargo git rpm-build tar xz
	./scripts/package-linux-rpm.sh --version "$(VERSION)" --rpm-release "$(RPM_RELEASE)" --out "$(outdir)"
