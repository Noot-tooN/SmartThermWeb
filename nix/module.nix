{ config, lib, pkgs, ... }:
let
  cfg = config.services.smartthermweb;
in {
  options.services.smartthermweb = {
    enable = lib.mkEnableOption "SmartThermWeb";
    package = lib.mkOption {
      type = lib.types.package;
      default = pkgs.callPackage ./package.nix { };
      description = "SmartThermWeb package, including the embedded frontend.";
    };
    port = lib.mkOption {
      type = lib.types.port;
      default = 3000;
      description = "Localhost port for the HTTPS reverse proxy.";
    };
    publicOrigin = lib.mkOption {
      type = lib.types.str;
      example = "https://heating.homeserver.com";
      description = "Exact HTTPS browser origin, without a trailing slash or path.";
    };
  };
  config = lib.mkIf cfg.enable {
    assertions = [{
      assertion = builtins.match "https://[^/@?#]+" cfg.publicOrigin != null;
      message = "services.smartthermweb.publicOrigin must be an HTTPS origin without a path.";
    }];
    systemd.services.smartthermweb = {
      description = "SmartThermWeb thermostat dashboard";
      wantedBy = [ "multi-user.target" ];
      wants = [ "network-online.target" ];
      after = [ "network-online.target" ];
      environment = {
        SMARTTHERM_PROVIDER = "ocstat";
        SMARTTHERM_BIND = "127.0.0.1:${toString cfg.port}";
        SMARTTHERM_PUBLIC_ORIGIN = cfg.publicOrigin;
      };
      serviceConfig = {
        ExecStart = lib.getExe cfg.package;
        DynamicUser = true;
        Restart = "on-failure";
        RestartSec = 5;
        # A schedule save can need up to 370 seconds. Let it finish during shutdown.
        TimeoutStopSec = 400;
        NoNewPrivileges = true;
        ProtectSystem = "strict";
        ProtectHome = true;
        PrivateTmp = true;
        PrivateDevices = true;
        ProtectKernelTunables = true;
        ProtectKernelModules = true;
        ProtectControlGroups = true;
        RestrictSUIDSGID = true;
        LockPersonality = true;
        RestrictAddressFamilies = [ "AF_INET" "AF_INET6" "AF_UNIX" ];
        CapabilityBoundingSet = "";
        UMask = "0077";
      };
    };
  };
}
