# Home-manager module for Coco Voice speech-to-text
#
# Provides a systemd user service for autostart.
# Usage: imports = [ coco-voice.homeManagerModules.default ];
#        services.coco-voice.enable = true;
{
  config,
  lib,
  pkgs,
  ...
}:
let
  cfg = config.services.coco-voice;
in
{
  options.services.coco-voice = {
    enable = lib.mkEnableOption "Coco Voice speech-to-text user service";

    package = lib.mkOption {
      type = lib.types.package;
      defaultText = lib.literalExpression "coco-voice.packages.\${system}.coco-voice";
      description = "The Coco Voice package to use.";
    };
  };

  config = lib.mkIf cfg.enable {
    systemd.user.services.coco-voice = {
      Unit = {
        Description = "Coco Voice speech-to-text";
        After = [ "graphical-session.target" ];
        PartOf = [ "graphical-session.target" ];
      };
      Service = {
        ExecStart = "${cfg.package}/bin/coco-voice";
        Restart = "on-failure";
        RestartSec = 5;
      };
      Install.WantedBy = [ "graphical-session.target" ];
    };
  };
}
