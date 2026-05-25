#!/usr/bin/env sh
set -eu

repo="dikmri/fvCapture"
version="${FVCAPTURE_VERSION:-latest}"
install_dir="${FVCAPTURE_INSTALL_DIR:-$HOME/.local/share/fvCapture}"
bin_dir="${FVCAPTURE_BIN_DIR:-$HOME/.local/bin}"

case "$(uname -s)" in
    Darwin*) asset="fvCapture-macos.tar.gz" ;;
    Linux*) asset="fvCapture-linux-x86_64.tar.gz" ;;
    *)
        echo "Unsupported OS: $(uname -s)" >&2
        exit 1
        ;;
esac

if [ "$version" = "latest" ]; then
    download_url="https://github.com/$repo/releases/latest/download/$asset"
else
    download_url="https://github.com/$repo/releases/download/$version/$asset"
fi

tmp_dir="$(mktemp -d)"
archive="$tmp_dir/$asset"
extract_dir="$tmp_dir/extract"
cleanup() {
    rm -rf "$tmp_dir"
}
trap cleanup EXIT INT TERM

mkdir -p "$extract_dir" "$install_dir" "$bin_dir"

echo "Downloading $asset..."
if command -v curl >/dev/null 2>&1; then
    curl -fL "$download_url" -o "$archive"
elif command -v wget >/dev/null 2>&1; then
    wget -O "$archive" "$download_url"
else
    echo "curl or wget is required." >&2
    exit 1
fi

echo "Extracting to $install_dir..."
tar -xzf "$archive" -C "$extract_dir"
cp -R "$extract_dir"/. "$install_dir"/
chmod +x "$install_dir/fvCapture" "$install_dir/fv-capture"

ln -sf "$install_dir/fvCapture" "$bin_dir/fvCapture"
ln -sf "$install_dir/fv-capture" "$bin_dir/fv-capture"

echo "fvCapture installed to $install_dir"
echo "Run: fvCapture"
case ":$PATH:" in
    *":$bin_dir:"*) ;;
    *)
        echo "Add $bin_dir to PATH if the command is not found."
        ;;
esac
