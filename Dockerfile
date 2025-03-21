FROM archlinux:base-devel
WORKDIR /usr/hpx

COPY . .
RUN mkdir -p $HOME/.hpx/out

RUN pacman -Syu --noconfirm
# alsa should not be needed anymore
RUN pacman -S clang neovim rustup alsa-utils lua bpf netcat --noconfirm
RUN rustup default stable
RUN bpftool btf dump file /sys/kernel/btf/vmlinux format c > $HOME/.hpx/out/vmlinux.h
RUN cargo run -- generate -c config/hpx_docker.json
RUN clang -O2 -g -target bpf -c $HOME/.hpx/out/generated.c -o $HOME/.hpx/out/generated.o

EXPOSE 8080
