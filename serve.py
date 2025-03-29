import web
import os
import markdown
from datetime import datetime
import re
from jinja2 import Environment, FileSystemLoader

# Define URLs and their corresponding classes
urls = (
    '/', 'Index',
    '/post/(.*)', 'Post',
    '/static/(.*)', 'Static'
)

# Directory where markdown files are stored
POSTS_DIR = 'posts'

# Set up Jinja2 environment
env = Environment(loader=FileSystemLoader('templates'))

# Function to read posts from markdown files
def get_posts():
    posts = []
    for filename in os.listdir(POSTS_DIR):
        if filename.endswith('.html'):
            filepath = os.path.join(POSTS_DIR, filename)
            with open(filepath, 'r') as f:
                # Read the first line as the title
                title = f.readline().strip()
                # Read the rest of the file as content
                content = f.read()
                # Parse the date from the filename in YYYYMMDD format
                date_str = filename.split('_')[0]
                date = datetime.strptime(date_str, '%Y%m%d').strftime('%Y-%m-%d')
                # Generate a slug from the title
                slug = re.sub(r'[^a-zA-Z0-9]+', '-', title.lower()).strip('-')
                posts.append({'title': title, 'content': content, 'date': date, 'slug': slug})
    # Sort posts by date, newest first
    posts.sort(key=lambda x: x['date'], reverse=True)
    return posts

# Define the Index class to handle the homepage
class Index:
    def GET(self):
        posts = get_posts()
        template = env.get_template('index.html')
        return template.render(posts=posts)

# Define the Post class to handle individual blog posts
class Post:
    def GET(self, slug):
        posts = get_posts()
        post = next((p for p in posts if p['slug'] == slug), None)
        if post:
            template = env.get_template('post.html')
            return template.render(post=post)
        else:
            return "Post not found."

# Class to serve static files
class Static:
    def GET(self, filename):
        try:
            with open(os.path.join('static', filename), 'rb') as f:
                return f.read()
        except FileNotFoundError:
            return web.notfound()

# Create the application
app = web.application(urls, globals())
app = app.wsgifunc()

# Use StaticMiddleware to serve static files
app = web.httpserver.StaticMiddleware(app)

if __name__ == "__main__":
    web.httpserver.runsimple(app, ("0.0.0.0", 8080))
