FROM debian:latest

RUN apt-get update && apt-get install -y locales && rm -rf /var/lib/apt/lists/* \
    && localedef -i en_US -c -f UTF-8 -A /usr/share/locale/locale.alias en_US.UTF-8
ENV LANG en_US.utf8
RUN apt-get update && apt-get install -y \ 
    build-essential curl git python3 libssl-dev unzip protobuf-compiler pkg-config
RUN curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y
RUN curl -fsSL https://deno.land/install.sh | sh
