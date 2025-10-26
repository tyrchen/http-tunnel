# Instructions

## Analyze existing lambda code

Please carefully review existing lambda code again @specs/serverless/0004-lambda.md spec, see if implementation is complete. Also see if proper dynamodb table is built. Add enough log info for better observability. Make a concrete plan rather than try this and that once you satisfied with the plan, execute

## Fix subpath issue

another problem - if accessing <https://tunnel.example.com/whsxs3svzbxw/docs>, the html page contains links like "/openapi.json", "/docs/oauth2-redirect", those won't work for sub path solution. We may need to do content rewrite to make the "/xxx" links prefixed with conn-id to make it "/<conn-id>/xxx". Think thoroughly on this solution and plan to implement it and document it in @specs/serverless/0009-content-rewrite.md

## Allow subdomain for tunnel

we want to allow users to use subdomain for tunnel, like <https://tunnel.example.com/whsxs3svzbxw/docs>, we want to allow users to use <https://whsxs3svzbxw.tunnel.example.com/docs> as well. The lambda function shall be able to handle both cases. And for subdomain case it shall be able to extract the subdomain from the url and use it to lookup the connection in the database and skip content rewrite. Look into existing impl, think ultra hard and document the plan in @specs/serverless/0012-subdomain.md
