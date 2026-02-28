FROM ubuntu:24.04

ENV DEBIAN_FRONTEND=noninteractive

RUN apt-get update \
    && apt-get install -y --no-install-recommends \
        ca-certificates \
        iproute2 \
        openssh-client \
        openssh-server \
        procps \
        xclip \
        xvfb \
    && rm -rf /var/lib/apt/lists/*

RUN mkdir -p /run/sshd /root/.ssh \
    && chmod 700 /root/.ssh \
    && ssh-keygen -A \
    && printf '%s\n' \
        'PermitRootLogin yes' \
        'PubkeyAuthentication yes' \
        'PasswordAuthentication no' \
        'UsePAM no' \
        'X11Forwarding no' \
        >> /etc/ssh/sshd_config

RUN printf '%s\n' \
    '#!/bin/sh' \
    'if [ "$1" = "ip" ] && [ "$2" = "-4" ]; then' \
    '  ip -4 -o addr show dev eth0 | awk '\''{print $4}'\'' | cut -d/ -f1 | head -n1' \
    '  exit 0' \
    'fi' \
    'exit 1' \
    > /usr/local/bin/tailscale \
    && chmod +x /usr/local/bin/tailscale

CMD ["bash", "-lc", "/usr/sbin/sshd && sleep infinity"]
