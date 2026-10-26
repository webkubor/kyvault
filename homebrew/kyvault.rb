# typed: strict
# frozen_string_literal: true

# Local-first encrypted secret and API key manager.
class Kyvault < Formula
  desc "Local-first encrypted secret and API key manager"
  homepage "https://github.com/webkubor/kyvault"
  version "2.7.0"
  license "MIT"

  on_macos do
    on_arm do
      url "https://github.com/webkubor/kyvault/releases/download/v2.7.0/kyvault-aarch64-apple-darwin.tar.gz"
      sha256 "167777f0339eb95fc84921f32e39637a27fc1aa9d01ee19917f532f5cf6001fc"
    end
    on_intel do
      url "https://github.com/webkubor/kyvault/releases/download/v2.7.0/kyvault-x86_64-apple-darwin.tar.gz"
      sha256 "5c2e981b2a2773c9f49539af851b90b794c81ef3b980cb1e89f860212bdb7c2a"
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
