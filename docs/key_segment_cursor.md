this file is out of date. the issues are solved and the design changed a bit since then.
it's only kept for historical reasons

I'm trying to create a visual representation of what I think the key_segment_cursor does




[graphical representaion]
I will mark elements that are highlighted/selected like <element>
I'll only show the case with one of the "with children" scope

I'll always try to show the representation in the file and how it's rendered
the filename is stripping.properties in the examples folder

This is the starting state. The user just navigated to firstmessage


http.status=status
http.status.200=OK
http.status.400=Bad Request
http.status.401=Unauthorized
http.status.403=Forbidden
http.status.404=Not Found
http.status.500=Internal Server Error
http.status.detail.something.msg.<firstmessage>=firstmessage
http.status.detail.something.msg.secondmessage=secondmessage
http.status.detail.something.notmsg=notmsg

stripping:[default]
  http.status: [default] status
    .200: [default] OK
    .400: [default] Bad Request
    .401: [default] Unauthorized
    .403: [default] Forbidden
    .404: [default] Not Found
    .500: [default] Internal Server Error
    .detail.something:[default]
      .msg:[default]
        .<firstmessage>: [default] firstmessage
        .secondmessage: [default] secondmessage
      .notmsg: [default] notmsg


now the user pressed ctrl+left

http.status=status
http.status.200=OK
http.status.400=Bad Request
http.status.401=Unauthorized
http.status.403=Forbidden
http.status.404=Not Found
http.status.500=Internal Server Error
http.status.detail.something.<msg>.<firstmessage>=firstmessage
http.status.detail.something.<msg>.<secondmessage>=secondmessage
http.status.detail.something.notmsg=notmsg

stripping:[default]
  http.status: [default] status
    .200: [default] OK
    .400: [default] Bad Request
    .401: [default] Unauthorized
    .403: [default] Forbidden
    .404: [default] Not Found
    .500: [default] Internal Server Error
    .detail.something:[default]
      .<msg>:[default]
        .<firstmessage>: [default] firstmessage
        .<secondmessage>: [default] secondmessage
      .notmsg: [default] notmsg

now the user pressed ctrl+left a second time

http.status=status
http.status.200=OK
http.status.400=Bad Request
http.status.401=Unauthorized
http.status.403=Forbidden
http.status.404=Not Found
http.status.500=Internal Server Error
http.status.detail.<something>.<msg>.<firstmessage>=firstmessage
http.status.detail.<something>.<msg>.<secondmessage>=secondmessage
http.status.detail.<something>.<notmsg>=notmsg

stripping:[default]
  http.status: [default] status
    .200: [default] OK
    .400: [default] Bad Request
    .401: [default] Unauthorized
    .403: [default] Forbidden
    .404: [default] Not Found
    .500: [default] Internal Server Error
    .detail.<something>:[default]
      .<msg>:[default]
        .<firstmessage>: [default] firstmessage
        .<secondmessage>: [default] secondmessage
      .<notmsg>: [default] notmsg

again ctrl+left pressed

http.status=status
http.status.200=OK
http.status.400=Bad Request
http.status.401=Unauthorized
http.status.403=Forbidden
http.status.404=Not Found
http.status.500=Internal Server Error
http.status.<detail>.<something>.<msg>.<firstmessage>=firstmessage
http.status.<detail>.<something>.<msg>.<secondmessage>=secondmessage
http.status.<detail>.<something>.<notmsg>=notmsg

stripping:[default]
  http.status: [default] status
    .200: [default] OK
    .400: [default] Bad Request
    .401: [default] Unauthorized
    .403: [default] Forbidden
    .404: [default] Not Found
    .500: [default] Internal Server Error
    .<detail>.<something>:[default]
      .<msg>:[default]
        .<firstmessage>: [default] firstmessage
        .<secondmessage>: [default] secondmessage
      .<notmsg>: [default] notmsg

again ctrl+left

http.<status>=status
http.<status>.<200>=OK
http.<status>.<400>=Bad Request
http.<status>.<401>=Unauthorized
http.<status>.<403>=Forbidden
http.<status>.<404>=Not Found
http.<status>.<500>=Internal Server Error
http.<status>.<detail>.<something>.<msg>.<firstmessage>=firstmessage
http.<status>.<detail>.<something>.<msg>.<secondmessage>=secondmessage
http.<status>.<detail>.<something>.<notmsg>=notmsg

stripping:[default]
  http.<status>: [default] status
    .<200>: [default] OK
    .<400>: [default] Bad Request
    .<401>: [default] Unauthorized
    .<403>: [default] Forbidden
    .<404>: [default] Not Found
    .<500>: [default] Internal Server Error
    .<detail>.<something>:[default]
      .<msg>:[default]
        .<firstmessage>: [default] firstmessage
        .<secondmessage>: [default] secondmessage
      .<notmsg>: [default] notmsg

again ctrl+left

<http>.<status>=status
<http>.<status>.<200>=OK
<http>.<status>.<400>=Bad Request
<http>.<status>.<401>=Unauthorized
<http>.<status>.<403>=Forbidden
<http>.<status>.<404>=Not Found
<http>.<status>.<500>=Internal Server Error
<http>.<status>.<detail>.<something>.<msg>.<firstmessage>=firstmessage
<http>.<status>.<detail>.<something>.<msg>.<secondmessage>=secondmessage
<http>.<status>.<detail>.<something>.<notmsg>=notmsg

stripping:[default]
  <http>.<status>: [default] status
    .<200>: [default] OK
    .<400>: [default] Bad Request
    .<401>: [default] Unauthorized
    .<403>: [default] Forbidden
    .<404>: [default] Not Found
    .<500>: [default] Internal Server Error
    .<detail>.<something>:[default]
      .<msg>:[default]
        .<firstmessage>: [default] firstmessage
        .<secondmessage>: [default] secondmessage
      .<notmsg>: [default] notmsg


[sometime the key_segment_cursor doesn't move]

I'm actually no longer sure if that problem really exists or if there are problems
with the rendering/highlighting. Since highlighting currently doesn't take keysegment boundaries
into account you sometimes have to press ctrl+left multiple times to see an effect.
I'll check this again when we ironed out the visual representation
