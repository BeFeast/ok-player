FROM ubuntu@sha256:4fbb8e6a8395de5a7550b33509421a2bafbc0aab6c06ba2cef9ebffbc7092d90

ENV DEBIAN_FRONTEND=noninteractive
ENV DOTNET_ROOT=/opt/dotnet
ENV PATH=/root/.cargo/bin:/opt/dotnet:/root/.dotnet/tools:${PATH}

COPY linux-candidate-toolchain.manifest /tmp/linux-candidate-toolchain.manifest

RUN apt-get update -qq \
    && packages="$(awk -F'|' '!/^#/ && NF == 4 { print $4 }' /tmp/linux-candidate-toolchain.manifest | sort -u | tr '\n' ' ')" \
    && apt-get install -y -qq --no-install-recommends \
      $packages \
      appstream appstream-compose ca-certificates curl dpkg-dev \
      libadwaita-1-dev libegl1-mesa-dev libgl1-mesa-dev libglx-dev \
      libgtk-4-dev libmpv-dev squashfs-tools unzip xz-utils \
    && rm -rf /var/lib/apt/lists/*

RUN curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs \
      | sh -s -- -y --profile minimal --default-toolchain stable \
    && curl --proto '=https' --tlsv1.2 -sSfL https://dot.net/v1/dotnet-install.sh \
      -o /tmp/dotnet-install.sh \
    && bash /tmp/dotnet-install.sh --channel 9.0 --install-dir /opt/dotnet \
    && /opt/dotnet/dotnet tool install -g vpk --version 1.2.0 \
    && rm -f /tmp/dotnet-install.sh

WORKDIR /workspace
