# typed: strict
# frozen_string_literal: true

# Local-first encrypted secret and API key manager.
class Kyvault < Formula
  desc "Local-first encrypted secret and API key manager"
  homepage "https://github.com/webkubor/kyvault"
  version "2.7.1"
  license "MIT"

  on_macos do
    on_arm do
      url "https://github.com/webkubor/kyvault/releases/download/v2.7.0/kyvault-aarch64-apple-darwin.tar.gz"
      sha256 "1544f86ee49b05da060eb04b0581f9ab58a4bc3f16937ddc6f83dcaf72c5347f"
    end
    on_intel do
      url "https://github.com/webkubor/kyvault/releases/download/v2.7.0/kyvault-x86_64-apple-darwin.tar.gz"
      sha256 "9850c5ee28b10c8d20341659b164f85523bc9c5efa211c8dad3b1167d9caf07e"
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
