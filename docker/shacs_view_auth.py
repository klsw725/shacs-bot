"""websockify auth plugin of the shacs-bot screen viewer (Spec 050 PRD 010).

A WebSocket connection passes only when its Origin, without the scheme, is
the Host it was sent to. Static files are not WebSocket connections and are
not checked.
"""

from websockify.auth_plugins import AuthenticationError


class SameOrigin:
    def __init__(self, src=None):
        self.source = src

    def authenticate(self, headers, target_host, target_port):
        origin = headers.get("Origin")
        host = headers.get("Host")
        if not origin or not host:
            raise AuthenticationError(response_code=403)
        scheme, separator, authority = origin.partition("://")
        if not scheme or not separator or authority != host:
            raise AuthenticationError(response_code=403)
