#!/bin/sh

build() {
  cd "$HOME" || exit 1
  if [ -d "rsspal" ]; then
    cd rsspal || exit 1
    git pull origin main
  else
    rm -f "rsspal"
    git clone "https://github.com/aftix/rsspal" "rsspal"
    cd rsspal || exit 1
  fi

  cargo build --release
}

build

sudo systemctl disable --now rsspal.service || true
sudo install -D "$BUILDHOME/rsspal/target/rsspal" /usr/local/bin
sudo install -D "$BUILDHOME/rsspal/rsspal.sh" /usr/local/bin
sudo install -D -m 644 "$BUILDHOME/rsspal/rsspal.service" /usr/lib/systemd/system
sudo systemctl daemon-reload

echo "$DISCORD_TOKEN" | sudo systemd-creds encrypt --name=token - /mnt/www/discord_token.cred
sudo systemctl enable --now rsspal.service
