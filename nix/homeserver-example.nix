# Import this alongside SmartThermWeb's NixOS module. No deployment is performed by this file.
{ inputs, pkgs, ... }:
{
  imports = [ inputs.smartthermweb.nixosModules.default ];
  services.smartthermweb = {
    enable = true;
    package = inputs.smartthermweb.packages.${pkgs.stdenv.hostPlatform.system}.default;
    publicOrigin = "https://heating.homeserver.com";
    port = 3000;
  };
  services.caddy = {
    enable = true;
    virtualHosts."heating.homeserver.com".extraConfig = ''
      tls internal
      reverse_proxy 127.0.0.1:3000 {
        # Flush SSE feedback immediately; schedule saves can take several minutes.
        flush_interval -1
        transport http {
          response_header_timeout 400s
        }
      }
    '';
  };
}
