# typed: strict
# frozen_string_literal: true

# Local-first encrypted secret and API key manager.
class Kyvault < Formula
  desc "Local-first encrypted secret and API key manager"
  homepage "https://github.com/webkubor/kyvault"
  version "2.6.0"
  license "MIT"

  on_macos do
    on_arm do
      url "https://github.com/webkubor/kyvault/releases/download/v2.6.0/kyvault-aarch64-apple-darwin.tar.gz"
      sha256 "6d482268677d878d4cc8351b4e9173811f2be54105f402e5766b2f01942fbb75"
    end
    on_intel do
      url "https://github.com/webkubor/kyvault/releases/download/v2.6.0/kyvault-x86_64-apple-darwin.tar.gz"
      sha256 "40ce65c32fd7b6b859e9f4ccbde1aaba79b44f8dca1ecd61517d6d8b4934c4f1"
    end
  end

  def install
    bin.install "kyvault"
    bin.install_symlink "kyvault" => "ky"
  end

  test do
    assert_match version.to_s, shell_output("#{bin}/kyvault --version")
  end
end
